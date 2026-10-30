//! Strategies for `aldis-spp` (133.0; VER-054).
//!
//! Header fields, the Maximum Packet Length configuration, whole Space Packets built by the
//! crate's own encoder, Idle Packets, tracker and segmentation inputs, and packet octets next to
//! the decoder's boundaries.
//!
//! The `time` feature of `aldis-spp` is not enabled here: enabling it from this dev-only crate
//! would enable it in every test build of `aldis-spp` (GEN-006).

use aldis_core::ids::{Apid, PacketVersion, SeqCount};
use aldis_spp::{
    IdlePacket, MAX_PACKET_LEN, MIN_PACKET_LEN, PacketData, PacketFields, PacketType,
    SequenceFlags, SppConfig, encode,
};
use proptest::prelude::*;

use super::common::{apid, packet_version_other_than, seq_count, user_apid};
use super::{octets, proper_prefix, with_trailing};

/// Telemetry or telecommand (133.0 §4.1.3.3.2, SPP-009).
pub fn packet_type() -> impl Strategy<Value = PacketType> {
    prop_oneof![Just(PacketType::Telemetry), Just(PacketType::Telecommand)]
}

/// Any of the four Sequence Flags (133.0 §4.1.3.4.2; SPP-012, SPP-013).
pub fn sequence_flags() -> impl Strategy<Value = SequenceFlags> {
    (0_u8..4).prop_map(SequenceFlags::from_bits)
}

/// Header fields with any APID, the idle APID included (133.0 §4.1.3; SPP-002, API-021).
pub fn packet_fields() -> impl Strategy<Value = PacketFields> {
    fields_with(apid())
}

/// Header fields with a user APID, never the idle APID (133.0 §4.1.3.3.4; SPP-002, SPP-010).
pub fn user_packet_fields() -> impl Strategy<Value = PacketFields> {
    fields_with(user_apid())
}

/// Header fields with APIDs from `apid`.
fn fields_with(apid: impl Strategy<Value = Apid>) -> impl Strategy<Value = PacketFields> {
    (packet_type(), apid, sequence_flags(), seq_count()).prop_map(
        |(packet_type, apid, sequence_flags, sequence_count)| PacketFields {
            packet_type,
            apid,
            sequence_flags,
            sequence_count,
        },
    )
}

/// A valid Maximum Packet Length configuration, 7–65 542 octets, the edges included (133.0
/// Table 5-1; SPP-006, GEN-060).
pub fn spp_config() -> impl Strategy<Value = SppConfig> {
    prop_oneof![
        1 => prop_oneof![
            Just(MIN_PACKET_LEN),
            Just(MIN_PACKET_LEN + 1),
            Just(MAX_PACKET_LEN - 1),
            Just(MAX_PACKET_LEN),
        ],
        3 => MIN_PACKET_LEN..=MAX_PACKET_LEN,
    ]
    .prop_filter_map("length in range", |len| SppConfig::new(len).ok())
}

/// Maximum Packet Lengths that `SppConfig::new` rejects: below 7 or above 65 542 octets, the
/// values next to the range drawn most often (133.0 §4.1.2.2; SPP-006, GEN-060).
pub fn max_packet_len_out_of_range() -> BoxedStrategy<usize> {
    prop_oneof![
        2 => prop_oneof![Just(MIN_PACKET_LEN - 1), Just(MAX_PACKET_LEN + 1)],
        1 => 0..MIN_PACKET_LEN,
        1 => MAX_PACKET_LEN + 1..=usize::MAX,
    ]
    .boxed()
}

/// A Space Packet built by `SppConfig::packet` and `encode`, with the parts it was built from
/// (133.0 §4.1; SPP-002, SPP-005, API-020, API-021).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedSpacePacket {
    /// The configuration the packet was checked against.
    pub config: SppConfig,
    /// The sender-chosen header fields.
    pub fields: PacketFields,
    /// The Packet Secondary Header; empty for none.
    pub secondary_header: Vec<u8>,
    /// The User Data Field.
    pub user_data: Vec<u8>,
    /// The encoded packet.
    pub octets: Vec<u8>,
}

impl EncodedSpacePacket {
    /// The checked parts, as `encode` takes them (SPP-005, RS-020).
    ///
    /// # Panics
    /// Never for a packet from these strategies, whose parts were checked when it was built.
    #[must_use]
    pub fn data(&self) -> PacketData<'_> {
        self.config
            .packet(self.fields, &self.secondary_header, &self.user_data)
            .expect("checked when built")
    }

    /// The Packet Data Field: the secondary header followed by the user data (133.0 §4.1.4).
    #[must_use]
    pub fn data_field(&self) -> Vec<u8> {
        [self.secondary_header.as_slice(), self.user_data.as_slice()].concat()
    }
}

/// A packet with `fields`, encoded under the default configuration (133.0 §4.1.4; SPP-005,
/// SPP-006).
///
/// It has a secondary header of `0..max_secondary` octets (none for the idle APID) and
/// `0..max_user` octets of user data, of which at least one part is non-empty.
///
/// # Panics
/// If `max_user` is 1 or less while no secondary header is possible, which leaves only empty
/// data fields.
pub fn space_packet_with(
    fields: impl Strategy<Value = PacketFields>,
    max_secondary: usize,
    max_user: usize,
) -> impl Strategy<Value = EncodedSpacePacket> {
    (fields, octets(0..max_secondary), octets(0..max_user)).prop_filter_map(
        "a data field",
        |(fields, sec, user)| {
            let secondary_header = if fields.apid.is_idle() {
                Vec::new()
            } else {
                sec
            };
            build(SppConfig::DEFAULT, fields, secondary_header, user)
        },
    )
}

/// A packet with any fields, a secondary header of up to 7 octets, and up to 39 octets of user
/// data ([`space_packet_with`]; SPP-002, SPP-005, API-020).
pub fn space_packet() -> impl Strategy<Value = EncodedSpacePacket> {
    space_packet_with(packet_fields(), 8, 40)
}

/// The packet, or `None` if the parts are rejected (an empty data field).
fn build(
    config: SppConfig,
    fields: PacketFields,
    secondary_header: Vec<u8>,
    user_data: Vec<u8>,
) -> Option<EncodedSpacePacket> {
    let data = config.packet(fields, &secondary_header, &user_data).ok()?;
    let mut out = vec![0_u8; data.packet_len()];
    let n = encode(&data, &mut out).ok()?;
    out.truncate(n);
    Some(EncodedSpacePacket {
        config,
        fields,
        secondary_header,
        user_data,
        octets: out,
    })
}

/// An Idle Packet of 7–70 octets with any sequence count and a pattern of 1–3 octets, encoded
/// by `IdlePacket` (133.0 §4.1.3.3.4.4, §4.1.4.3.4; SPP-010).
pub fn idle_packet() -> impl Strategy<Value = Vec<u8>> {
    (MIN_PACKET_LEN..=70, seq_count(), octets(1..4)).prop_filter_map(
        "an encodable Idle Packet",
        |(len, count, pattern)| {
            let p = IdlePacket::new(&SppConfig::DEFAULT, len)
                .ok()?
                .with_sequence_count(count)
                .with_pattern(&pattern);
            let mut out = vec![0_u8; len];
            p.encode(&mut out).ok()?;
            Some(out)
        },
    )
}

/// One input of a `SequenceTracker`: a count for an APID, or forgetting that APID (SPP-011,
/// CMN-031).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackerEvent {
    /// The APID.
    pub apid: Apid,
    /// The count received.
    pub count: SeqCount,
    /// Forget the APID instead of tracking the count.
    pub forget: bool,
}

/// Up to `max_events - 1` tracker inputs over the APIDs `0..apids`, so that a small tracker
/// fills up and APIDs recur (SPP-011, CMN-031).
///
/// # Panics
/// If `apids` is 0 or above 2048.
pub fn tracker_events(apids: u16, max_events: usize) -> impl Strategy<Value = Vec<TrackerEvent>> {
    assert!((1..=Apid::MAX + 1).contains(&apids), "1-2048 APIDs");
    let event =
        (0..apids, seq_count(), any::<bool>()).prop_filter_map("an APID", |(a, count, forget)| {
            Some(TrackerEvent {
                apid: Apid::new(a)?,
                count,
                forget,
            })
        });
    proptest::collection::vec(event, 0..max_events)
}

/// The inputs of a segmentation (133.0 §4.1.3.4.2, SPP-013).
///
/// A user APID, an SDU of 1–199 octets, a maximum user data length of 1–39 octets per segment,
/// the first count, and a secondary header of 0–3 octets for every segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segmentation {
    /// Packet type of the segments.
    pub packet_type: PacketType,
    /// APID of the segments; never idle.
    pub apid: Apid,
    /// The service data unit to segment.
    pub sdu: Vec<u8>,
    /// Most user data octets per segment.
    pub max_user: usize,
    /// Sequence count of the first segment.
    pub start: SeqCount,
    /// Secondary header of every segment; empty for none.
    pub secondary_header: Vec<u8>,
}

/// A [`Segmentation`] (133.0 §4.1.3.4.2, SPP-013).
pub fn segmentation() -> impl Strategy<Value = Segmentation> {
    (
        packet_type(),
        user_apid(),
        octets(1..200),
        1_usize..40,
        seq_count(),
        octets(0..4),
    )
        .prop_map(
            |(packet_type, apid, sdu, max_user, start, secondary_header)| Segmentation {
                packet_type,
                apid,
                sdu,
                max_user,
                start,
                secondary_header,
            },
        )
}

// ----- Packet octets next to the decoder's boundaries -----

/// Octet 0 bits of the primary header (133.0 §4.1.3.2–3): the Packet Version Number in bits
/// 0–2 and the Secondary Header Flag in bit 4.
const VERSION_MASK: u8 = 0b1110_0000;
const SECONDARY_HEADER_FLAG: u8 = 0b0000_1000;
/// The Packet Data Length occupies octets 4–5 of the primary header (133.0 §4.1.3.5).
const LENGTH_FIELD: core::ops::Range<usize> = 4..6;

/// `packet` with its Packet Data Length changed by `delta` (133.0 §4.1.3.5, SPP-002).
fn with_length_delta(mut packet: Vec<u8>, delta: i16) -> Vec<u8> {
    let field = &mut packet[LENGTH_FIELD];
    let length = u16::from_be_bytes([field[0], field[1]]);
    let changed = length.wrapping_add_signed(delta);
    field.copy_from_slice(&changed.to_be_bytes());
    packet
}

/// Space Packet octets next to the boundaries of `SpacePacket::decode` (133.0 §4.1; GEN-020,
/// API-030, VER-023):
///
/// - a valid packet or Idle Packet, possibly followed by other octets;
/// - a proper prefix of one: one octet too short, or cut inside the primary header (SPP-002);
/// - the Packet Data Length one more or one less than the data present (SPP-002);
/// - a Packet Version Number other than `000` (SPP-001);
/// - an Idle Packet with the Secondary Header Flag set (SPP-004).
pub fn space_packet_near_boundary() -> BoxedStrategy<Vec<u8>> {
    let valid = prop_oneof![3 => space_packet().prop_map(|p| p.octets), 1 => idle_packet()];
    prop_oneof![
        valid.clone().prop_flat_map(|v| with_trailing(v, 4)).boxed(),
        valid.clone().prop_flat_map(proper_prefix).boxed(),
        (valid.clone(), prop_oneof![Just(1), Just(-1)])
            .prop_map(|(v, d)| with_length_delta(v, d))
            .boxed(),
        (
            valid,
            packet_version_other_than(PacketVersion::SPACE_PACKET)
        )
            .prop_map(|(mut v, version)| {
                v[0] = (v[0] & !VERSION_MASK) | (version.get() << 5);
                v
            })
            .boxed(),
        idle_packet()
            .prop_map(|mut v| {
                v[0] |= SECONDARY_HEADER_FLAG;
                v
            })
            .boxed(),
    ]
    .boxed()
}

/// Input for a Space Packet decoder property: arbitrary octets or octets
/// [near a boundary](space_packet_near_boundary) (GEN-020, API-030, VER-023).
pub fn space_packet_decoder_input() -> BoxedStrategy<Vec<u8>> {
    prop_oneof![1 => octets(0..32), 3 => space_packet_near_boundary()].boxed()
}

#[cfg(test)]
mod tests {
    use super::{
        idle_packet, segmentation, space_packet, space_packet_decoder_input,
        space_packet_near_boundary, spp_config, tracker_events, user_packet_fields,
        with_length_delta,
    };
    use crate::strategies::config;
    use aldis_core::ErrorKind;
    use aldis_spp::{MAX_PACKET_LEN, MIN_PACKET_LEN, SpacePacket};
    use proptest::prelude::*;

    const REGRESSIONS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proptest-regressions/strategies.txt"
    );

    proptest! {
        #![proptest_config(config(256, REGRESSIONS))]

        #[test]
        fn packets_decode_strictly(p in space_packet(), idle in idle_packet()) {
            // req: VER-054, SPP-002, SPP-010
            let d = SpacePacket::decode(&p.octets).unwrap();
            prop_assert_eq!(d.header().fields(), p.fields);
            let data_field = p.data_field();
            prop_assert_eq!(d.data_field(), data_field.as_slice());
            prop_assert_eq!(p.data().packet_len(), p.octets.len());
            prop_assert!(SpacePacket::decode(&idle).unwrap().is_idle());
        }

        #[test]
        fn length_off_by_one_is_noticed(p in space_packet()) {
            // req: VER-054, SPP-002
            let longer = with_length_delta(p.octets.clone(), 1);
            prop_assert_eq!(
                SpacePacket::decode(&longer).unwrap_err().kind(),
                ErrorKind::Incomplete
            );
            if p.octets.len() > MIN_PACKET_LEN {
                let shorter = with_length_delta(p.octets, -1);
                let d = SpacePacket::decode(&shorter).unwrap();
                prop_assert_eq!(d.len() + 1, shorter.len());
            }
        }

        #[test]
        fn other_strategies_are_in_range(
            c in spp_config(),
            f in user_packet_fields(),
            events in tracker_events(4, 10),
            s in segmentation(),
            a in space_packet_near_boundary(),
            b in space_packet_decoder_input(),
        ) {
            // req: VER-054, GEN-020
            prop_assert!((MIN_PACKET_LEN..=MAX_PACKET_LEN).contains(&c.max_packet_len()));
            prop_assert!(!f.apid.is_idle() && !s.apid.is_idle());
            prop_assert!(events.iter().all(|e| e.apid.get() < 4));
            prop_assert!(!s.sdu.is_empty() && s.max_user >= 1);
            for bytes in [a, b] {
                let _ = SpacePacket::decode(&bytes);
                let _ = SpacePacket::decode_lenient(&bytes);
            }
        }
    }
}
