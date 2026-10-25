//! Strategies for `aldis-uslp` (732.1; VER-054).
//!
//! A fixed-length Physical Channel (one data VC with an Insert Zone, and VC 63 for OID frames)
//! or a variable-length one (a VC with truncated frames and an OCF allowed, a VC with SDLS
//! lengths per MAP, and a VC with a fixed-length TFDZ); frames of every kind built by the crate's
//! own encoders; data units of several frames and fill for the delimiter; and frame octets next
//! to the decoder's boundaries.

use aldis_core::ocf::Ocf;
use aldis_core::oid::OidGenerator;
use aldis_core::security::SecurityLayout;
use aldis_uslp::{
    ChannelFields, ConstructionRule, FrameContent, FrameFields, FrameKind, MapConfig, OcfUse,
    PhysicalChannelConfig, SecurityScope, SourceOrDestination, TfdfHeader, TfdzOffset, TfdzRule,
    TransferFrameType, TruncatedFrameLen, Upid, UslpMapId, UslpSpacecraftId, UslpVcId, VcConfig,
    VcLookup, VcParams, VcTable, VcfCount, VcfCountLen, encode, encode_oid, encode_truncated,
};
use proptest::prelude::*;

use super::common::{map_id, spacecraft_id, up_to_u16, up_to_u64, vc_id};
use super::{octets, proper_prefix, with_trailing};

/// The channel table of these strategies.
pub type Channels = VcTable<4, 8>;

/// The VCID of OID frames.
const OID: u8 = 63;

/// Any frame kind of Table 4-1 (USLP-001).
pub fn frame_kind() -> impl Strategy<Value = FrameKind> {
    prop_oneof![
        Just(FrameKind::SequenceControlled),
        Just(FrameKind::Expedited),
        Just(FrameKind::ProtocolControl),
    ]
}

/// Any UPID, assigned or not (USLP-007).
pub fn upid() -> impl Strategy<Value = Upid> {
    (0_u8..32).prop_filter_map("5 bits", Upid::from_bits)
}

/// A VCF Count Length, 0–7 octets (USLP-004).
pub fn vcf_count_len() -> impl Strategy<Value = VcfCountLen> {
    (0_u8..=7).prop_filter_map("0-7", |n| VcfCountLen::new(n).ok())
}

/// A VCF Count of any length, 0–7 octets, and any value the length holds, its edges drawn more
/// often (732.1 §4.1.2.11–12, Table 4-2; USLP-004).
pub fn vcf_count() -> impl Strategy<Value = VcfCount> {
    vcf_count_len().prop_flat_map(|len| {
        up_to_u64(len.max_count())
            .prop_filter_map("fits the length", move |v| VcfCount::new(len, v))
    })
}

/// Any TFDZ offset, 0 to `0xFFFE`, the edges drawn more often (732.1 §4.1.4.2.4.3–6; USLP-006,
/// RS-020).
pub fn tfdz_offset() -> impl Strategy<Value = TfdzOffset> {
    up_to_u16(TfdzOffset::MAX).prop_filter_map("in range", TfdzOffset::new)
}

/// A Security Header of 0–6 and a Trailer of 0–4 octets (API-070).
fn small_security() -> impl Strategy<Value = SecurityLayout> {
    (0_usize..=6, 0_usize..=4).prop_filter_map("in range", |(h, t)| SecurityLayout::new(h, t).ok())
}

/// A Physical Channel and its channels, with the data VCs and their MAPs (732.1 §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UslpChannels {
    /// The table.
    pub table: Channels,
    /// The Spacecraft ID of every VC.
    pub spacecraft_id: UslpSpacecraftId,
    /// The VCs other than 63 with their MAPs, at least one each.
    pub vcs: Vec<(UslpVcId, Vec<UslpMapId>)>,
    /// Whether VC 63 is configured for OID frames.
    pub oid: bool,
}

/// A fixed-length channel: frames of 32–256 octets, an Insert Zone of 0–4 octets, a data VC
/// (VCF Count Lengths, OCF required or absent, SDLS) with 1–3 MAPs, and VC 63 (USLP-003,
/// USLP-008, USLP-009).
fn fixed_channels() -> impl Strategy<Value = UslpChannels> {
    (
        (32_usize..=256, 0_usize..=4, any::<bool>()),
        spacecraft_id::<16>(),
        vc_id::<6>(),
        (
            vcf_count_len(),
            vcf_count_len(),
            any::<bool>(),
            small_security(),
        ),
        proptest::collection::btree_set(map_id::<4>(), 1..=3),
        (vcf_count_len(), vcf_count_len(), any::<bool>()),
    )
        .prop_filter_map(
            "a valid fixed channel",
            |((len, iz, fecf), scid, vcid, (seq, exp, ocf, sec), maps, (oseq, oexp, oocf))| {
                (vcid.get() != OID).then_some(())?;
                let pc =
                    PhysicalChannelConfig::new(TransferFrameType::Fixed, len, iz, fecf).ok()?;
                let use_of = |b| if b { OcfUse::Required } else { OcfUse::Absent };
                let data = VcParams {
                    vcf_count_len_sequence: seq,
                    vcf_count_len_expedited: exp,
                    ocf: use_of(ocf),
                    security: SecurityScope::Vc(sec),
                    ..VcParams::new(TransferFrameType::Fixed)
                };
                let oid = VcParams {
                    vcf_count_len_sequence: oseq,
                    vcf_count_len_expedited: oexp,
                    ocf: use_of(oocf),
                    ..VcParams::new(TransferFrameType::Fixed)
                };
                let vcs = [
                    VcConfig::new(scid, vcid, data).ok()?,
                    VcConfig::new(scid, UslpVcId::new(OID)?, oid).ok()?,
                ];
                let map_list: Vec<UslpMapId> = maps.into_iter().collect();
                let configs: Vec<MapConfig> = map_list
                    .iter()
                    .map(|&m| MapConfig::new(scid, vcid, m, SecurityLayout::NONE))
                    .collect();
                Some(UslpChannels {
                    table: Channels::new(pc, &vcs, &configs).ok()?,
                    spacecraft_id: scid,
                    vcs: vec![(vcid, map_list)],
                    oid: true,
                })
            },
        )
}

/// A variable-length channel of at most 64–512 octets: VC `a` (OCF allowed or not, truncated
/// frames of 6–32 octets or none, no SDLS), VC `b` (SDLS per MAP, truncated frames of 28–32
/// octets or none), and VC `c` with a fixed-length TFDZ (USLP-002, USLP-012, USLP-016).
fn variable_channels() -> impl Strategy<Value = UslpChannels> {
    (
        (64_usize..=512, any::<bool>()),
        spacecraft_id::<16>(),
        (vc_id::<6>(), vc_id::<6>(), vc_id::<6>()),
        (
            vcf_count_len(),
            vcf_count_len(),
            any::<bool>(),
            proptest::option::of(6_usize..=32),
        ),
        (proptest::option::of(28_usize..=32), small_security()),
        (vcf_count_len(), vcf_count_len()),
        map_id::<4>(),
    )
        .prop_filter_map(
            "a valid variable channel",
            |(
                (max, fecf),
                scid,
                (a, b, c),
                (seq, exp, ocf, trunc_a),
                (trunc_b, sec_b),
                (cs, ce),
                m,
            )| {
                let ids = [a.get(), b.get(), c.get()];
                (ids.iter().all(|&v| v != OID) && a != b && b != c && a != c).then_some(())?;
                let pc =
                    PhysicalChannelConfig::new(TransferFrameType::Variable, max, 0, fecf).ok()?;
                let trunc = |t: Option<usize>| t.map(TruncatedFrameLen::new).transpose();
                let va = VcParams {
                    vcf_count_len_sequence: seq,
                    vcf_count_len_expedited: exp,
                    ocf: if ocf { OcfUse::Allowed } else { OcfUse::Absent },
                    truncated_frame_len: trunc(trunc_a).ok()?,
                    ..VcParams::new(TransferFrameType::Variable)
                };
                let vb = VcParams {
                    truncated_frame_len: trunc(trunc_b).ok()?,
                    security: SecurityScope::PerMap,
                    ..VcParams::new(TransferFrameType::Variable)
                };
                let vc = VcParams {
                    vcf_count_len_sequence: cs,
                    vcf_count_len_expedited: ce,
                    ..VcParams::new(TransferFrameType::Fixed)
                };
                let vcs = [
                    VcConfig::new(scid, a, va).ok()?,
                    VcConfig::new(scid, b, vb).ok()?,
                    VcConfig::new(scid, c, vc).ok()?,
                ];
                let sec_b = if trunc_b.is_some() {
                    SecurityLayout::USLP_TRUNCATED
                } else {
                    sec_b
                };
                let maps = [
                    MapConfig::new(scid, a, m, SecurityLayout::NONE),
                    MapConfig::new(scid, b, m, sec_b),
                    MapConfig::new(scid, c, m, SecurityLayout::NONE),
                ];
                Some(UslpChannels {
                    table: Channels::new(pc, &vcs, &maps).ok()?,
                    spacecraft_id: scid,
                    vcs: vec![(a, vec![m]), (b, vec![m]), (c, vec![m])],
                    oid: false,
                })
            },
        )
}

/// A fixed-length or a variable-length Physical Channel and its channels (732.1 §5; GEN-060).
pub fn uslp_channels() -> impl Strategy<Value = UslpChannels> {
    prop_oneof![fixed_channels(), variable_channels()]
}

/// What a frame was built from (732.1 §4.1, Annex D).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameParts {
    /// A non-truncated frame of user data or protocol control commands.
    Full {
        /// The header fields.
        fields: FrameFields,
        /// The TFDF Header.
        tfdf: TfdfHeader,
        /// The Insert Zone.
        insert_zone: Vec<u8>,
        /// The OCF.
        ocf: Option<Ocf>,
    },
    /// A truncated frame.
    Truncated {
        /// The truncated header's fields.
        channel: ChannelFields,
        /// The TFDF Header.
        tfdf: TfdfHeader,
    },
    /// An OID frame from a generator that had produced `skip` octets before.
    Oid {
        /// The header fields.
        fields: FrameFields,
        /// The Insert Zone.
        insert_zone: Vec<u8>,
        /// The OCF.
        ocf: Option<Ocf>,
        /// Octets drawn from the generator before this frame.
        skip: usize,
    },
}

/// A frame built by the crate's encoders, with the parts it was built from (USLP-009, USLP-010,
/// API-020, API-021).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedUslpFrame {
    /// The channels the frame was checked against.
    pub channels: UslpChannels,
    /// What it was built from.
    pub parts: FrameParts,
    /// The TFDZ.
    pub tfdz: Vec<u8>,
    /// The encoded frame.
    pub octets: Vec<u8>,
}

/// A TFDZ rule of the VC's type with a pointer below `tfdz_len` or `0xFFFF` (USLP-005,
/// USLP-006, USLP-016).
fn rule_for(
    fixed: bool,
    pick: u8,
    pointer: Option<prop::sample::Index>,
    tfdz_len: usize,
) -> TfdzRule {
    let raw = pointer.map_or(0xFFFF, |i| {
        u16::try_from(i.index(tfdz_len)).unwrap_or(0xFFFF)
    });
    let bits = if fixed { pick % 3 } else { 3 + pick % 5 };
    TfdzRule::new(ConstructionRule::from_bits(bits), raw)
}

/// The TFDZ length that fills a fixed-length frame, or `None` (732.1 §4.1.4.1.2).
fn fixed_tfdz_len(c: &UslpChannels, fields: &FrameFields, ocf: bool) -> Option<usize> {
    let pc = c.table.physical_channel();
    let vc = c.table.vc(fields.spacecraft_id, fields.vc_id)?;
    let security = match (fields.kind, vc.security()) {
        (FrameKind::ProtocolControl, _) => SecurityLayout::NONE,
        (_, SecurityScope::Vc(l)) => l,
        (_, SecurityScope::PerMap) => c
            .table
            .map(fields.spacecraft_id, fields.vc_id, fields.map_id)?
            .security(),
    };
    let overhead = 7
        + usize::from(vc.vcf_count_len(fields.kind.bypass_flag()).get())
        + pc.insert_zone_len()
        + security.header_len()
        + 3
        + security.trailer_len()
        + if ocf { 4 } else { 0 }
        + if pc.fecf() { 2 } else { 0 };
    pc.frame_len().checked_sub(overhead).filter(|&n| n > 0)
}

/// Raw choices from which [`frame_on`] builds a frame.
type Choices = (
    (
        prop::sample::Index,
        prop::sample::Index,
        FrameKind,
        u64,
        bool,
    ),
    (u8, Option<prop::sample::Index>, Upid, [u8; 4], u8),
    (Vec<u8>, Vec<u8>, usize),
);

/// Any frame the encoders accept on `c` (732.1 §4.1, Annex D; USLP-002, USLP-009, USLP-010).
///
/// User data or protocol control frames on its data VCs, truncated frames where a VC has a
/// truncated frame length, and OID frames on VC 63.
pub fn frame_on(c: UslpChannels) -> impl Strategy<Value = EncodedUslpFrame> {
    let choices: (_, _, _) = (
        (
            any::<prop::sample::Index>(),
            any::<prop::sample::Index>(),
            frame_kind(),
            any::<u64>(),
            any::<bool>(),
        ),
        (
            any::<u8>(),
            proptest::option::of(any::<prop::sample::Index>()),
            upid(),
            any::<[u8; 4]>(),
            0_u8..4,
        ),
        (octets(0..=4), octets(1..=40), 0_usize..64),
    );
    choices.prop_filter_map("an encodable frame", move |ch: Choices| build(&c, ch))
}

/// The parts of a frame that every shape shares.
struct Common {
    fields: FrameFields,
    ocf_octets: [u8; 4],
    insert_zone: Vec<u8>,
}

/// Builds the frame of `choices` on `c`, or `None` if the encoders reject it.
fn build(c: &UslpChannels, choices: Choices) -> Option<EncodedUslpFrame> {
    let (
        (vc_pick, map_pick, kind, count, flag),
        (rule_pick, pointer, upid, ocf_octets, shape),
        (iz_seed, data, skip),
    ) = choices;
    let (vc_id, maps) = vc_pick.get(&c.vcs).clone();
    let vc = c.table.vc(c.spacecraft_id, vc_id)?;
    let insert_len = c.table.physical_channel().insert_zone_len();
    let common = Common {
        fields: FrameFields {
            spacecraft_id: c.spacecraft_id,
            source_or_destination: SourceOrDestination::from_bit(flag),
            vc_id,
            map_id: *map_pick.get(&maps),
            kind,
            vcf_count: count & vc.vcf_count_len(kind.bypass_flag()).max_count(),
        },
        ocf_octets,
        insert_zone: iz_seed.iter().copied().cycle().take(insert_len).collect(),
    };
    let common = if common.insert_zone.len() == insert_len {
        common
    } else {
        Common {
            insert_zone: vec![0; insert_len],
            ..common
        }
    };
    match shape {
        0 if c.oid => build_oid(c, common, skip),
        1 if vc.truncated_frame_len().is_some() => {
            let tfdf = TfdfHeader {
                rule: rule_for(false, rule_pick, None, 1),
                upid,
            };
            build_truncated(c, &common, tfdf, &data)
        }
        _ => {
            let ocf = match vc.ocf() {
                OcfUse::Absent => None,
                OcfUse::Required => Some(Ocf::decode(ocf_octets)),
                OcfUse::Allowed => flag.then(|| Ocf::decode(ocf_octets)),
            };
            let fixed = vc.frame_type() == TransferFrameType::Fixed;
            build_full(c, common, ocf, (fixed, rule_pick, pointer, upid), data)
        }
    }
}

/// An OID frame on VC 63 from a generator that drew `skip` octets before (USLP-009).
fn build_oid(c: &UslpChannels, common: Common, skip: usize) -> Option<EncodedUslpFrame> {
    let oid_id = UslpVcId::new(OID)?;
    let oid_vc = c.table.vc(c.spacecraft_id, oid_id)?;
    let kind = match common.fields.kind {
        FrameKind::ProtocolControl => FrameKind::Expedited,
        k => k,
    };
    let fields = FrameFields {
        vc_id: oid_id,
        map_id: UslpMapId::new(0)?,
        kind,
        vcf_count: common.fields.vcf_count & oid_vc.vcf_count_len(kind.bypass_flag()).max_count(),
        ..common.fields
    };
    let ocf = (oid_vc.ocf() == OcfUse::Required).then(|| Ocf::decode(common.ocf_octets));
    let mut generator = OidGenerator::new();
    generator.fill(&mut vec![0_u8; skip]);
    let mut out = vec![0_u8; 600];
    let insert_zone = common.insert_zone;
    let n = encode_oid(
        &c.table,
        fields,
        &insert_zone,
        ocf,
        &mut generator,
        &mut out,
    )
    .ok()?;
    out.truncate(n);
    let tfdz = aldis_uslp::UslpFrame::decode(&out, &c.table, aldis_core::DecodeMode::Strict)
        .ok()?
        .tfdz()
        .to_vec();
    Some(EncodedUslpFrame {
        channels: c.clone(),
        parts: FrameParts::Oid {
            fields,
            insert_zone,
            ocf,
            skip,
        },
        tfdz,
        octets: out,
    })
}

/// A truncated frame whose TFDZ fills its VC's truncated frame length (USLP-002, USLP-012).
fn build_truncated(
    c: &UslpChannels,
    common: &Common,
    tfdf: TfdfHeader,
    data: &[u8],
) -> Option<EncodedUslpFrame> {
    let f = common.fields;
    let len = c
        .table
        .vc(f.spacecraft_id, f.vc_id)?
        .truncated_frame_len()?
        .get();
    let channel = ChannelFields {
        spacecraft_id: f.spacecraft_id,
        source_or_destination: f.source_or_destination,
        vc_id: f.vc_id,
        map_id: f.map_id,
    };
    let sdls = c
        .table
        .map(f.spacecraft_id, f.vc_id, f.map_id)
        .is_some_and(|m| !m.security().is_none());
    let room = len.checked_sub(5 + if sdls { 22 } else { 0 })?;
    let zone: Vec<u8> = data.iter().copied().cycle().take(room).collect();
    let mut out = vec![0_u8; 64];
    let n = encode_truncated(&c.table, channel, tfdf, &zone, &mut out).ok()?;
    out.truncate(n);
    Some(EncodedUslpFrame {
        channels: c.clone(),
        parts: FrameParts::Truncated { channel, tfdf },
        tfdz: zone,
        octets: out,
    })
}

/// A non-truncated frame of user data or protocol control commands (USLP-010).
fn build_full(
    c: &UslpChannels,
    common: Common,
    ocf: Option<Ocf>,
    (fixed, rule_pick, pointer, upid): (bool, u8, Option<prop::sample::Index>, Upid),
    data: Vec<u8>,
) -> Option<EncodedUslpFrame> {
    let fields = common.fields;
    let zone: Vec<u8> = if c.table.physical_channel().frame_type() == TransferFrameType::Fixed {
        let n = fixed_tfdz_len(c, &fields, ocf.is_some())?;
        data.iter().copied().cycle().take(n).collect()
    } else {
        data
    };
    let tfdf = TfdfHeader {
        rule: rule_for(fixed, rule_pick, pointer, zone.len()),
        upid,
    };
    let insert_zone = common.insert_zone;
    let content = FrameContent {
        tfdf,
        insert_zone: &insert_zone,
        ocf,
    };
    let mut out = vec![0_u8; 600];
    let n = encode(&c.table, fields, content, &zone, &mut out).ok()?;
    out.truncate(n);
    Some(EncodedUslpFrame {
        channels: c.clone(),
        parts: FrameParts::Full {
            fields,
            tfdf,
            insert_zone,
            ocf,
        },
        tfdz: zone,
        octets: out,
    })
}

/// A frame of any kind on any [`uslp_channels`] (USLP-001…USLP-010).
pub fn uslp_frame() -> impl Strategy<Value = EncodedUslpFrame> {
    uslp_channels().prop_flat_map(frame_on)
}

/// One data unit of a delimiter: 1–4 frames on the same variable-length channels, then 0–6
/// octets of fill, `0x55` or arbitrary (USLP-015).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UslpDataUnit {
    /// The channels.
    pub channels: UslpChannels,
    /// The frames, in order.
    pub frames: Vec<Vec<u8>>,
    /// The fill after them.
    pub fill: Vec<u8>,
    /// The frames and the fill, concatenated.
    pub octets: Vec<u8>,
}

/// A [`UslpDataUnit`] (USLP-015).
pub fn data_unit() -> impl Strategy<Value = UslpDataUnit> {
    variable_channels()
        .prop_flat_map(|c| {
            (
                proptest::collection::vec(frame_on(c.clone()), 1..=4),
                prop_oneof![(0_usize..=6).prop_map(|n| vec![0x55; n]), octets(0..5)],
                Just(c),
            )
        })
        .prop_map(|(frames, fill, channels)| {
            let frames: Vec<Vec<u8>> = frames.into_iter().map(|f| f.octets).collect();
            let octets = [frames.concat(), fill.clone()].concat();
            UslpDataUnit {
                channels,
                frames,
                fill,
                octets,
            }
        })
}

/// Frame octets next to the boundaries of `UslpFrame::decode` and `validate_frame`.
///
/// With the channels (GEN-020, API-030, VER-023): a valid frame, possibly followed by other octets
/// (API-011); a proper prefix of one; its Frame Length one more or one less (USLP-003); a TFVN
/// changed, the reserved flags `01`, or the Reserve Spares set (USLP-001); the End of Frame
/// Primary Header Flag flipped (USLP-002); or one bit flipped.
pub fn uslp_frame_near_boundary() -> BoxedStrategy<(UslpChannels, Vec<u8>)> {
    let valid = uslp_frame().prop_map(|f| (f.channels, f.octets)).boxed();
    prop_oneof![
        valid
            .clone()
            .prop_flat_map(|(c, v)| (Just(c), with_trailing(v, 4)))
            .boxed(),
        valid
            .clone()
            .prop_flat_map(|(c, v)| (Just(c), proper_prefix(v)))
            .boxed(),
        (valid.clone(), prop_oneof![Just(1_u16), Just(0xFFFF)])
            .prop_map(|((c, mut v), d)| {
                if v.len() > 6 && v[3] & 1 == 0 {
                    let n = u16::from_be_bytes([v[4], v[5]]).wrapping_add(d);
                    v[4..6].copy_from_slice(&n.to_be_bytes());
                }
                (c, v)
            })
            .boxed(),
        (valid.clone(), 0_u8..16)
            .prop_map(|((c, mut v), version)| {
                v[0] = (v[0] & 0x0F) | (version << 4);
                (c, v)
            })
            .boxed(),
        (valid.clone(), 1_u8..4)
            .prop_map(|((c, mut v), bits)| {
                if v.len() > 6 && v[3] & 1 == 0 {
                    v[6] = (v[6] & 0x0F) | (bits << 4);
                }
                (c, v)
            })
            .boxed(),
        valid
            .clone()
            .prop_map(|(c, mut v)| {
                v[3] ^= 1;
                (c, v)
            })
            .boxed(),
        (valid, any::<prop::sample::Index>(), 0_u8..8)
            .prop_map(|((c, mut v), at, bit)| {
                let i = at.index(v.len());
                v[i] ^= 1 << bit;
                (c, v)
            })
            .boxed(),
    ]
    .boxed()
}

/// Input for a USLP decoder property: arbitrary octets or octets
/// [near a boundary](uslp_frame_near_boundary), with the channels (GEN-020, VER-023).
pub fn uslp_frame_decoder_input() -> BoxedStrategy<(UslpChannels, Vec<u8>)> {
    prop_oneof![
        1 => (uslp_channels(), octets(0..64)),
        3 => uslp_frame_near_boundary(),
    ]
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::{
        FrameParts, data_unit, tfdz_offset, uslp_frame, uslp_frame_decoder_input, vcf_count,
    };
    use crate::strategies::config;
    use aldis_core::DecodeMode;
    use aldis_uslp::{TfdzOffset, UslpFrame};
    use proptest::prelude::*;

    const REGRESSIONS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proptest-regressions/strategies.txt"
    );

    proptest! {
        #![proptest_config(config(64, REGRESSIONS))]

        #[test]
        fn frames_decode(f in uslp_frame()) {
            // req: VER-054
            let d = UslpFrame::decode(&f.octets, &f.channels.table, DecodeMode::Strict).unwrap();
            prop_assert_eq!(d.tfdz(), f.tfdz.as_slice());
            prop_assert_eq!(d.is_truncated(), matches!(f.parts, FrameParts::Truncated { .. }));
        }

        #[test]
        fn counts_and_offsets_are_in_range(c in vcf_count(), o in tfdz_offset()) {
            // req: VER-054, USLP-004, USLP-006
            prop_assert!(c.value() <= c.len().max_count());
            prop_assert!(o.get() <= TfdzOffset::MAX);
        }

        #[test]
        fn units_concatenate(u in data_unit()) {
            // req: VER-054
            prop_assert_eq!(u.octets.len(), u.frames.iter().map(Vec::len).sum::<usize>() + u.fill.len());
        }

        #[test]
        fn decoder_inputs_never_panic((c, v) in uslp_frame_decoder_input()) {
            // req: VER-054, GEN-020
            let _ = UslpFrame::decode(&v, &c.table, DecodeMode::Lenient);
        }
    }
}
