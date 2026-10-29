//! Strategies for `aldis-core`: identifiers, the decode mode, modular counters, bit fields,
//! frame layouts, and container operations (VER-054).
//!
//! Each identifier has a strategy for its valid values, with the edges of the range drawn more
//! often, and one for the raw values just past its maximum, which its checked constructor must
//! reject (API-001, API-002, RS-010).

use aldis_core::DecodeMode;
use aldis_core::counter::ModCounter;
use aldis_core::ids::{Apid, FrameSeqNum, MapId, PacketVersion, SeqCount, SpacecraftId, VcId};
use aldis_core::layout::LayoutSpec;
use aldis_core::security::SecurityLayout;
use proptest::prelude::*;

use super::octets;

/// Defines a strategy for `0..=max` of one unsigned type, with `0`, `1`, `max - 1`, and `max`
/// drawn more often than the values between.
macro_rules! up_to {
    ($(#[$doc:meta])* $name:ident, $ty:ty) => {
        $(#[$doc])*
        pub fn $name(max: $ty) -> BoxedStrategy<$ty> {
            prop_oneof![
                1 => prop_oneof![Just(0), Just(max.min(1)), Just(max.saturating_sub(1)), Just(max)],
                3 => 0..=max,
            ]
            .boxed()
        }
    };
}

up_to!(
    /// `0..=max` for 64-bit fields, the edges drawn more often (VER-054).
    up_to_u64, u64
);
up_to!(
    /// `0..=max` for 32-bit fields, the edges drawn more often (VER-054).
    up_to_u32, u32
);
up_to!(
    /// `0..=max` for 16-bit fields, the edges drawn more often (VER-054).
    up_to_u16, u16
);
up_to!(
    /// `0..=max` for 8-bit fields, the edges drawn more often (VER-054).
    up_to_u8, u8
);

/// `max + 1..=u16::MAX`, with `max + 1` drawn most often: raw values one past a 16-bit field's
/// range (API-001, GEN-041).
///
/// # Panics
/// If `max` is `u16::MAX`, which leaves nothing above it.
pub fn above_u16(max: u16) -> BoxedStrategy<u16> {
    assert!(max < u16::MAX, "no value above max");
    prop_oneof![1 => Just(max + 1), 1 => max + 1..=u16::MAX].boxed()
}

/// `max + 1..=u8::MAX` as [`above_u16`], for 8-bit fields.
///
/// # Panics
/// If `max` is `u8::MAX`.
pub fn above_u8(max: u8) -> BoxedStrategy<u8> {
    assert!(max < u8::MAX, "no value above max");
    prop_oneof![1 => Just(max + 1), 1 => max + 1..=u8::MAX].boxed()
}

/// Any Application Process Identifier, idle included (133.0 §4.1.3.3.4; API-001, SPP-002).
pub fn apid() -> impl Strategy<Value = Apid> {
    up_to_u16(Apid::MAX).prop_filter_map("in range", Apid::new)
}

/// Any APID but the idle APID 2047: the APIDs that carry user data (133.0 §4.1.3.3.4.4;
/// SPP-010, SPP-013).
pub fn user_apid() -> impl Strategy<Value = Apid> {
    up_to_u16(Apid::MAX - 1).prop_filter_map("in range", Apid::new)
}

/// Raw APID values above 2047, which `Apid::new` rejects (API-001, RS-010).
pub fn apid_out_of_range() -> BoxedStrategy<u16> {
    above_u16(Apid::MAX)
}

/// Any Packet Sequence Count (133.0 §4.1.3.4.3; API-001, SPP-011).
pub fn seq_count() -> impl Strategy<Value = SeqCount> {
    up_to_u16(SeqCount::MAX).prop_filter_map("in range", SeqCount::new)
}

/// Raw sequence counts above 16 383, which `SeqCount::new` rejects (API-001, RS-010).
pub fn seq_count_out_of_range() -> BoxedStrategy<u16> {
    above_u16(SeqCount::MAX)
}

/// Any 8-bit frame sequence number (CMN-031, API-001).
pub fn frame_seq_num() -> impl Strategy<Value = FrameSeqNum> {
    up_to_u8(FrameSeqNum::MAX).prop_map(FrameSeqNum::new)
}

/// Any Packet Version Number, 0–7 (133.0 §4.1.3.2, 133.1 §4.1.2.2; API-001).
pub fn packet_version() -> impl Strategy<Value = PacketVersion> {
    up_to_u8(PacketVersion::MAX).prop_filter_map("in range", PacketVersion::new)
}

/// Any Packet Version Number other than `version`: what a decoder for `version` rejects as
/// `UnsupportedVersion` (SPP-001, EPP-001).
pub fn packet_version_other_than(version: PacketVersion) -> impl Strategy<Value = PacketVersion> {
    packet_version().prop_filter("another version", move |v| *v != version)
}

/// Raw packet versions above 7, which `PacketVersion::new` rejects (API-001, RS-010).
pub fn packet_version_out_of_range() -> BoxedStrategy<u8> {
    above_u8(PacketVersion::MAX)
}

/// Any Spacecraft Identifier of `BITS` width (API-001, API-002).
pub fn spacecraft_id<const BITS: u32>() -> impl Strategy<Value = SpacecraftId<BITS>> {
    up_to_u16(SpacecraftId::<BITS>::MAX).prop_filter_map("in range", SpacecraftId::new)
}

/// Raw values just past a `BITS`-bit Spacecraft Identifier (API-002, RS-010).
///
/// # Panics
/// For `BITS` = 16, which has no value above its range in a `u16`.
pub fn spacecraft_id_out_of_range<const BITS: u32>() -> BoxedStrategy<u16> {
    above_u16(SpacecraftId::<BITS>::MAX)
}

/// Any Virtual Channel Identifier of `BITS` width (API-001, API-002).
pub fn vc_id<const BITS: u32>() -> impl Strategy<Value = VcId<BITS>> {
    up_to_u8(VcId::<BITS>::MAX).prop_filter_map("in range", VcId::new)
}

/// Raw values just past a `BITS`-bit VCID (API-002, RS-010).
///
/// # Panics
/// For `BITS` = 8.
pub fn vc_id_out_of_range<const BITS: u32>() -> BoxedStrategy<u8> {
    above_u8(VcId::<BITS>::MAX)
}

/// Any MAP Identifier of `BITS` width (API-001, API-002).
pub fn map_id<const BITS: u32>() -> impl Strategy<Value = MapId<BITS>> {
    up_to_u8(MapId::<BITS>::MAX).prop_filter_map("in range", MapId::new)
}

/// Raw values just past a `BITS`-bit MAP ID (API-002, RS-010).
///
/// # Panics
/// For `BITS` = 8.
pub fn map_id_out_of_range<const BITS: u32>() -> BoxedStrategy<u8> {
    above_u8(MapId::<BITS>::MAX)
}

/// Strict or lenient (API-030, API-031).
pub fn decode_mode() -> impl Strategy<Value = DecodeMode> {
    prop_oneof![Just(DecodeMode::Strict), Just(DecodeMode::Lenient)]
}

/// A counter width, 1–64 bits (CMN-030, GEN-024).
pub fn counter_width() -> impl Strategy<Value = u8> {
    1_u8..=64
}

/// A value of a `bits`-wide counter: `0..2^bits`, any `u64` masked to the width (CMN-030).
pub fn counter_value(bits: u8) -> impl Strategy<Value = u64> {
    let mask = u64::MAX >> (64 - u32::from(bits));
    any::<u64>().prop_map(move |v| v & mask)
}

/// A modular counter of any width 1–64 and a valid value (CMN-030, GEN-024).
pub fn mod_counter() -> impl Strategy<Value = ModCounter> {
    counter_width().prop_flat_map(|bits| {
        counter_value(bits).prop_filter_map("value fits", move |v| ModCounter::new(bits, v).ok())
    })
}

/// A modular counter and another value of the same width, for the distance and continuity of
/// two counts (CMN-030, CMN-031).
pub fn mod_counter_and_value() -> impl Strategy<Value = (ModCounter, u64)> {
    mod_counter().prop_flat_map(|c| (Just(c), counter_value(c.bits())))
}

/// A width and a value that `ModCounter::new` rejects: width 0, a width above 64, or a width of
/// 1–63 bits with a value that needs more bits (CMN-030, RS-020).
pub fn mod_counter_out_of_range() -> BoxedStrategy<(u8, u64)> {
    prop_oneof![
        1 => any::<u64>().prop_map(|v| (0, v)),
        1 => (65_u8..=u8::MAX, any::<u64>()),
        4 => (1_u8..64).prop_flat_map(|bits| {
            let first = 1_u64 << bits;
            (Just(bits), prop_oneof![Just(first), first..=u64::MAX])
        }),
    ]
    .boxed()
}

/// Four Operational Control Field octets, each of the four values of bits 0–1 drawn equally
/// often (132.0 §4.1.5.4–5, 732.0 §4.1.5.4–5, 732.1 §4.1.5.2; CMN-010).
pub fn ocf_octets() -> impl Strategy<Value = [u8; 4]> {
    (0_u8..4, any::<[u8; 4]>()).prop_map(|(flags, [first, rest @ ..])| {
        let [a, b, c] = rest;
        [(flags << 6) | (first & 0x3F), a, b, c]
    })
}

/// Three octet ranges for `SecuredFields::new` (355.0 §4.1; API-070, RS-020).
///
/// A Security Header, a data field, and a Security Trailer that are in order and adjacent, or
/// the same with one range end moved by one or two octets, which may reverse a range.
pub fn secured_ranges() -> BoxedStrategy<[core::ops::Range<usize>; 3]> {
    let adjacent = (0_usize..100, 0_usize..70, 0_usize..2000, 0_usize..40).prop_map(
        |(start, header, data, trailer)| {
            let data_start = start + header;
            let trailer_start = data_start + data;
            [
                start..data_start,
                data_start..trailer_start,
                trailer_start..trailer_start + trailer,
            ]
        },
    );
    let moved = (adjacent.clone(), 0_usize..6, 1_usize..=2, any::<bool>()).prop_map(
        |(mut r, bound, by, up)| {
            let edge = match bound % 6 {
                0 => &mut r[0].start,
                1 => &mut r[0].end,
                2 => &mut r[1].start,
                3 => &mut r[1].end,
                4 => &mut r[2].start,
                _ => &mut r[2].end,
            };
            *edge = if up {
                *edge + by
            } else {
                edge.saturating_sub(by)
            };
            r
        },
    );
    prop_oneof![1 => adjacent, 2 => moved].boxed()
}

/// A bit-field access: a buffer, a bit offset that may run past it, a width of 1–64 bits, and a
/// value that fits the width (GEN-050, GEN-051).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitFieldAccess {
    /// The buffer, 1–15 octets.
    pub buf: Vec<u8>,
    /// Bit offset from the most significant bit of `buf[0]`, 0–127; it may lie outside `buf`.
    pub offset: usize,
    /// Field width in bits, 1–64.
    pub width: u8,
    /// A value of at most `width` bits.
    pub value: u64,
}

/// A [`BitFieldAccess`], in range of its buffer or not (GEN-050, GEN-051).
pub fn bit_field_access() -> impl Strategy<Value = BitFieldAccess> {
    (octets(1..16), 0_usize..128, 1_u8..=64, any::<u64>()).prop_map(
        |(buf, offset, width, value)| BitFieldAccess {
            buf,
            offset,
            width,
            value: value & (u64::MAX >> (64 - u32::from(width))),
        },
    )
}

/// A Security Header of 0–64 octets and a Security Trailer of 0–39 octets (API-070, API-071).
/// The trailer is kept short so that frames stay small; its full range is in the unit tests.
pub fn security_layout() -> impl Strategy<Value = SecurityLayout> {
    (0..=SecurityLayout::MAX_HEADER_LEN, 0_usize..40)
        .prop_filter_map("lengths in range", |(h, t)| SecurityLayout::new(h, t).ok())
}

/// A frame layout specification, valid or with a frame too short for its fixed fields (CMN-040,
/// CMN-041).
///
/// Frame length 0–1999, primary header 0–15, secondary slot 0–69, a [`security_layout`], and
/// OCF and FECF present or not.
pub fn layout_spec() -> impl Strategy<Value = LayoutSpec> {
    (
        0_usize..2000,
        0_usize..16,
        0_usize..70,
        security_layout(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(frame_len, primary_header_len, secondary_len, security, ocf, fecf)| LayoutSpec {
                frame_len,
                primary_header_len,
                secondary_len,
                security,
                ocf,
                fecf,
            },
        )
}

/// One operation on a record queue such as `ByteQueue` (GEN-010, API-061).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueOp {
    /// Push a record with these octets.
    Push(Vec<u8>),
    /// Pop the oldest record.
    Pop,
    /// Discard the oldest record.
    Discard,
}

/// Up to `max_ops - 1` queue operations; pushed records have 0 to `max_record - 1` octets
/// (GEN-010, API-061).
pub fn byte_queue_ops(max_record: usize, max_ops: usize) -> impl Strategy<Value = Vec<QueueOp>> {
    let op = prop_oneof![
        octets(0..max_record).prop_map(QueueOp::Push),
        Just(QueueOp::Pop),
        Just(QueueOp::Discard),
    ];
    proptest::collection::vec(op, 0..max_ops)
}

/// Up to `max_ops - 1` operations on a fixed queue of octets: `Some(v)` pushes `v`, `None` pops
/// (GEN-010).
pub fn fixed_queue_ops(max_ops: usize) -> impl Strategy<Value = Vec<Option<u8>>> {
    proptest::collection::vec(proptest::option::of(any::<u8>()), 0..max_ops)
}

#[cfg(test)]
mod tests {
    use super::{
        above_u8, above_u16, apid, apid_out_of_range, bit_field_access, map_id,
        map_id_out_of_range, mod_counter_and_value, packet_version_other_than,
        packet_version_out_of_range, seq_count_out_of_range, spacecraft_id,
        spacecraft_id_out_of_range, up_to_u16, user_apid, vc_id, vc_id_out_of_range,
    };
    use crate::strategies::config;
    use aldis_core::ids::{Apid, MapId, PacketVersion, SeqCount, SpacecraftId, VcId};
    use proptest::prelude::*;

    const REGRESSIONS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proptest-regressions/strategies.txt"
    );

    proptest! {
        #![proptest_config(config(128, REGRESSIONS))]

        #[test]
        fn bounded_values_stay_in_range(
            (max, v) in any::<u16>().prop_flat_map(|max| (Just(max), up_to_u16(max)))
        ) {
            // req: VER-054
            prop_assert!(v <= max);
        }

        #[test]
        fn out_of_range_values_are_rejected(
            a in apid_out_of_range(),
            s in seq_count_out_of_range(),
            p in packet_version_out_of_range(),
            sc in spacecraft_id_out_of_range::<10>(),
            vc in vc_id_out_of_range::<3>(),
            map in map_id_out_of_range::<6>(),
            b in above_u8(200),
            w in above_u16(60_000),
        ) {
            // req: VER-054, API-001, API-002
            prop_assert!(Apid::new(a).is_none());
            prop_assert!(SeqCount::new(s).is_none());
            prop_assert!(PacketVersion::new(p).is_none());
            prop_assert!(SpacecraftId::<10>::new(sc).is_none());
            prop_assert!(VcId::<3>::new(vc).is_none());
            prop_assert!(MapId::<6>::new(map).is_none());
            prop_assert!(b > 200 && w > 60_000);
        }

        #[test]
        fn valid_values_are_valid(
            a in apid(),
            u in user_apid(),
            v in packet_version_other_than(PacketVersion::SPACE_PACKET),
            sc in spacecraft_id::<8>(),
            vc in vc_id::<6>(),
            map in map_id::<4>(),
            (c, next) in mod_counter_and_value(),
            access in bit_field_access(),
        ) {
            // req: VER-054
            prop_assert!(a.get() <= Apid::MAX && !u.is_idle());
            prop_assert_ne!(v, PacketVersion::SPACE_PACKET);
            prop_assert!(sc.get() <= 255 && vc.get() <= 63 && map.get() <= 15);
            prop_assert!(c.distance_to(next).is_ok());
            prop_assert!(access.width >= 1 && access.value <= u64::MAX >> (64 - u32::from(access.width)));
        }
    }
}
