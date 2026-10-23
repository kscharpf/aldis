//! Strategies for `aldis-tc` (232.0; VER-054).
//!
//! Frame types and header fields, Control Commands, Segment Headers, a Physical Channel with one
//! Virtual Channel without and one with Segment Headers (and SDLS lengths on both), whole frames
//! built by the crate's own encoder, data units of several frames and fill for the delimiter,
//! CLCWs, and frame octets next to the decoder's boundaries.

use aldis_core::ids::FrameSeqNum;
use aldis_core::security::SecurityLayout;
use aldis_tc::{
    Clcw, ClcwStatus, ControlCommand, CopInEffect, FarmBCounter, FrameData, FrameFields, FrameType,
    MapConfig, PhysicalChannelConfig, SegmentHeader, SegmentHeaderUse, SequenceFlags, TcMapId,
    TcVcId, VcConfig, VcTable, encode,
};
use proptest::prelude::*;

use super::common::{frame_seq_num, map_id, spacecraft_id, vc_id};
use super::{octets, proper_prefix, with_trailing};

/// The channel table of these strategies: room for two Virtual Channels and four MAPs.
pub type Channels = VcTable<2, 4>;

/// Type-AD, Type-BD, or Type-BC (232.0 Table 4-1; TC-002).
pub fn frame_type() -> impl Strategy<Value = FrameType> {
    prop_oneof![
        Just(FrameType::Ad),
        Just(FrameType::Bd),
        Just(FrameType::Bc)
    ]
}

/// Any of the four Sequence Flags (232.0 Table 4-2; TC-004).
pub fn sequence_flags() -> impl Strategy<Value = SequenceFlags> {
    (0_u8..4).prop_map(SequenceFlags::from_bits)
}

/// Unlock, or Set V(R) with any value (232.0 §4.1.3.3; TC-005).
pub fn control_command() -> impl Strategy<Value = ControlCommand> {
    prop_oneof![
        Just(ControlCommand::Unlock),
        frame_seq_num().prop_map(ControlCommand::SetVr),
    ]
}

/// Any CLCW (232.0 §4.2.1; TC-030, TC-031).
pub fn clcw() -> impl Strategy<Value = Clcw> {
    (
        (0_u8..8, 0_u8..4, vc_id::<6>(), 0_u8..4, frame_seq_num()),
        proptest::array::uniform5(any::<bool>()),
    )
        .prop_filter_map(
            "fields in range",
            |((status, cop, vc_id, farm_b, report_value), [rf, bl, lo, wa, re])| {
                Some(Clcw {
                    status: ClcwStatus::new(status)?,
                    cop_in_effect: CopInEffect::new(cop)?,
                    vc_id,
                    no_rf_available: rf,
                    no_bit_lock: bl,
                    lockout: lo,
                    wait: wa,
                    retransmit: re,
                    farm_b_counter: FarmBCounter::new(farm_b)?,
                    report_value,
                })
            },
        )
}

/// A Security Header of 0–8 octets and a Security Trailer of 0–4 octets: small, so frames stay
/// short (API-070).
fn small_security() -> impl Strategy<Value = SecurityLayout> {
    (0_usize..=8, 0_usize..=4)
        .prop_filter_map("lengths in range", |(h, t)| SecurityLayout::new(h, t).ok())
}

/// A Physical Channel and its channels, with the identifiers the frames use (232.0 §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcChannels {
    /// The table.
    pub table: Channels,
    /// Fields of a frame on the Virtual Channel without Segment Headers (Type-AD, N(S) 0).
    pub plain: FrameFields,
    /// Fields of a frame on the Virtual Channel with Segment Headers (Type-AD, N(S) 0).
    pub segmented: FrameFields,
    /// The MAPs of the segmented Virtual Channel, at least one.
    pub maps: Vec<TcMapId>,
}

/// A Physical Channel with two Virtual Channels (232.0 §5, §6.6; GEN-060, API-070).
///
/// Frames of at most 64, 200, or 1024 octets, with or without FECF; one Virtual Channel without
/// Segment Headers and one with them and 1–4 MAPs, each with a small SDLS layout.
pub fn tc_channels() -> impl Strategy<Value = TcChannels> {
    (
        prop_oneof![Just(64_usize), Just(200), Just(1024)],
        any::<bool>(),
        spacecraft_id::<10>(),
        (vc_id::<6>(), vc_id::<6>()),
        small_security(),
        proptest::collection::btree_map(map_id::<6>(), small_security(), 1..=4),
    )
        .prop_filter_map(
            "two Virtual Channels",
            |(max, fecf, scid, (a, b), plain_security, maps)| {
                (a != b).then_some(())?;
                let pc = PhysicalChannelConfig::new(max, fecf).ok()?;
                let vcs = [
                    VcConfig::new(scid, a, 1024, SegmentHeaderUse::Absent(plain_security)).ok()?,
                    VcConfig::new(scid, b, 1024, SegmentHeaderUse::Present).ok()?,
                ];
                let map_configs: Vec<MapConfig> = maps
                    .iter()
                    .map(|(&m, &s)| MapConfig::new(scid, b, m, s))
                    .collect();
                let table = Channels::new(pc, &vcs, &map_configs).ok()?;
                let fields = |vc_id: TcVcId| FrameFields {
                    frame_type: FrameType::Ad,
                    spacecraft_id: scid,
                    vc_id,
                    sequence_number: FrameSeqNum::new(0),
                };
                Some(TcChannels {
                    table,
                    plain: fields(a),
                    segmented: fields(b),
                    maps: maps.keys().copied().collect(),
                })
            },
        )
}

/// A frame built by `encode`, with the parts it was built from (232.0 §4.1; TC-001, TC-004,
/// TC-005, TC-007, API-020, API-021).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedTcFrame {
    /// The channels the frame was checked against.
    pub channels: TcChannels,
    /// The header fields.
    pub fields: FrameFields,
    /// The Segment Header of a Type-D frame on the segmented Virtual Channel.
    pub segment_header: Option<SegmentHeader>,
    /// The Control Command of a Type-BC frame.
    pub control: Option<ControlCommand>,
    /// The User Data of a Type-D frame; empty for a Type-BC frame.
    pub user_data: Vec<u8>,
    /// The encoded frame.
    pub octets: Vec<u8>,
}

impl EncodedTcFrame {
    /// The data field content, as `encode` takes it (TC-004, TC-005).
    #[must_use]
    pub fn data(&self) -> FrameData<'_> {
        match self.control {
            Some(command) => FrameData::Control(command),
            None => FrameData::Fdu {
                segment_header: self.segment_header,
                user_data: &self.user_data,
            },
        }
    }
}

/// A frame of any type on [`tc_channels`] (232.0 §4.1; TC-008).
///
/// Type-AD or Type-BD with 1–40 octets of User Data on either Virtual Channel (with a Segment
/// Header and one of its MAPs on the segmented one), or Type-BC with a [`control_command`];
/// any N(S).
pub fn tc_frame() -> impl Strategy<Value = EncodedTcFrame> {
    (
        tc_channels(),
        frame_type(),
        any::<bool>(),
        frame_seq_num(),
        sequence_flags(),
        any::<prop::sample::Index>(),
        control_command(),
        octets(1..=40),
    )
        .prop_filter_map(
            "an encodable frame",
            |(channels, frame_type, on_segmented, ns, flags, map, command, user)| {
                let base = if on_segmented {
                    channels.segmented
                } else {
                    channels.plain
                };
                let fields = FrameFields {
                    frame_type,
                    sequence_number: ns,
                    ..base
                };
                let control = (frame_type == FrameType::Bc).then_some(command);
                let segment_header = (on_segmented && control.is_none()).then(|| SegmentHeader {
                    sequence_flags: flags,
                    map_id: *map.get(&channels.maps),
                });
                let user_data = if control.is_some() { Vec::new() } else { user };
                let mut frame = EncodedTcFrame {
                    channels,
                    fields,
                    segment_header,
                    control,
                    user_data,
                    octets: Vec::new(),
                };
                let mut out = vec![0_u8; 1024];
                let n = encode(&frame.channels.table, fields, frame.data(), &mut out).ok()?;
                out.truncate(n);
                frame.octets = out;
                Some(frame)
            },
        )
}

/// One data unit of a delimiter: 1–4 frames on the same channels, then 0–6 octets of fill,
/// either `0x55` (232.0 §4.4.9.2.6 note 2) or arbitrary octets that cannot start a valid header
/// (TC-020).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataUnit {
    /// The frames, in order.
    pub frames: Vec<Vec<u8>>,
    /// The fill after them.
    pub fill: Vec<u8>,
    /// The frames and the fill, concatenated.
    pub octets: Vec<u8>,
}

/// A [`DataUnit`] of frames from [`tc_frame`] (TC-020).
pub fn data_unit() -> impl Strategy<Value = DataUnit> {
    (
        proptest::collection::vec(tc_frame(), 1..=4),
        prop_oneof![(0_usize..=6).prop_map(|n| vec![0x55; n]), octets(0..5),],
    )
        .prop_map(|(frames, fill)| {
            let frames: Vec<Vec<u8>> = frames.into_iter().map(|f| f.octets).collect();
            let octets = [frames.concat(), fill.clone()].concat();
            DataUnit {
                frames,
                fill,
                octets,
            }
        })
}

// ----- Frame octets next to the decoder's boundaries -----

/// `frame` with its Frame Length changed by `delta`, wrapping within its 10 bits (232.0
/// §4.1.2.7; TC-001).
fn with_length_delta(mut frame: Vec<u8>, delta: i16) -> Vec<u8> {
    let count = u16::from_be_bytes([frame[2] & 0b11, frame[3]]).wrapping_add_signed(delta) & 0x3FF;
    let [hi, lo] = count.to_be_bytes();
    frame[2] = (frame[2] & !0b11) | hi;
    frame[3] = lo;
    frame
}

/// Frame octets next to the boundaries of `TcFrame::decode` and `validate_frame`, with the
/// channels they are decoded against (232.0 §4.1, §4.4.9.3; GEN-020, API-030, VER-023):
///
/// - a valid frame, possibly followed by other octets (API-011);
/// - a proper prefix of one (TC-001);
/// - its Frame Length one more or one less (TC-001, TC-021 d);
/// - a version other than `00`, the reserved flags `01`, or a Reserved Spare set (TC-002,
///   TC-003);
/// - one bit of the frame flipped, which the FECF catches when present (TC-006).
pub fn tc_frame_near_boundary() -> BoxedStrategy<(TcChannels, Vec<u8>)> {
    let valid = tc_frame().prop_map(|f| (f.channels, f.octets)).boxed();
    prop_oneof![
        valid
            .clone()
            .prop_flat_map(|(c, v)| (Just(c), with_trailing(v, 4)))
            .boxed(),
        valid
            .clone()
            .prop_flat_map(|(c, v)| (Just(c), proper_prefix(v)))
            .boxed(),
        (valid.clone(), prop_oneof![Just(1_i16), Just(-1)])
            .prop_map(|((c, v), d)| (c, with_length_delta(v, d)))
            .boxed(),
        (valid.clone(), 1_u8..4)
            .prop_map(|((c, mut v), version)| {
                v[0] = (v[0] & 0x3F) | (version << 6);
                (c, v)
            })
            .boxed(),
        valid
            .clone()
            .prop_map(|(c, mut v)| {
                v[0] = (v[0] & 0xCF) | 0x10;
                (c, v)
            })
            .boxed(),
        (valid.clone(), 1_u8..4)
            .prop_map(|((c, mut v), spare)| {
                v[0] = (v[0] & 0xF3) | (spare << 2);
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

/// Input for a TC frame decoder property: arbitrary octets or octets
/// [near a boundary](tc_frame_near_boundary), with the channels (GEN-020, VER-023).
pub fn tc_frame_decoder_input() -> BoxedStrategy<(TcChannels, Vec<u8>)> {
    prop_oneof![
        1 => (tc_channels(), octets(0..64)),
        3 => tc_frame_near_boundary(),
    ]
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::{clcw, data_unit, tc_frame, tc_frame_decoder_input, with_length_delta};
    use crate::strategies::config;
    use aldis_core::DecodeMode;
    use aldis_tc::{FrameType, TcFrame};
    use proptest::prelude::*;

    const REGRESSIONS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proptest-regressions/strategies.txt"
    );

    #[test]
    fn length_delta_wraps_in_ten_bits() {
        // req: VER-054
        let v = with_length_delta(vec![0x01, 0x55, 0x07, 0xFF, 0], 1);
        assert_eq!(&v[2..4], [0x04, 0x00]);
        let v = with_length_delta(v, -1);
        assert_eq!(&v[2..4], [0x07, 0xFF]);
    }

    proptest! {
        #![proptest_config(config(64, REGRESSIONS))]

        #[test]
        fn frames_decode(f in tc_frame()) {
            // req: VER-054
            let d = TcFrame::decode(&f.octets, &f.channels.table, DecodeMode::Strict).unwrap();
            prop_assert_eq!(d.header().fields(), f.fields);
            prop_assert_eq!(d.control_command(), f.control);
            prop_assert_eq!(f.control.is_some(), f.fields.frame_type == FrameType::Bc);
        }

        #[test]
        fn units_concatenate(u in data_unit(), c in clcw()) {
            // req: VER-054
            prop_assert_eq!(u.octets.len(), u.frames.iter().map(Vec::len).sum::<usize>() + u.fill.len());
            prop_assert!(u.fill.len() <= 6);
            prop_assert_eq!(c.to_octets()[0] & 0xE0, 0);
        }

        #[test]
        fn decoder_inputs_never_panic((c, v) in tc_frame_decoder_input()) {
            // req: VER-054, GEN-020
            let _ = TcFrame::decode(&v, &c.table, DecodeMode::Lenient);
        }
    }
}
