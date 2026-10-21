//! Strategies for `aldis-epp` (133.1; VER-054).
//!
//! Protocol identifiers, header fields and sizes, managed parameters, whole Encapsulation
//! Packets built by the crate's own encoder, Idle Packets, and packet octets next to the
//! decoder's boundaries.

use aldis_core::ids::PacketVersion;
use aldis_epp::config::MAX_DATA_LEN;
use aldis_epp::{
    DataUnit, EncapsulationFields, Epi, EpiExtension, EpiSet, EppConfig, ExtendedEpiSet, HeaderLen,
    HeaderSize, UserDefined, encode, idle,
};
use proptest::prelude::*;

use super::common::{packet_version_other_than, up_to_u32};
use super::{octets, proper_prefix, with_trailing};

/// Any Encapsulation Protocol ID, idle `000` included (133.1 §4.1.2.3, EPP-008).
pub fn epi() -> impl Strategy<Value = Epi> {
    (0..=Epi::MAX).prop_filter_map("in range", Epi::new)
}

/// Any Protocol ID Extension (133.1 §4.1.2.6; EPP-006, EPP-008).
pub fn epi_extension() -> impl Strategy<Value = EpiExtension> {
    (0..=EpiExtension::MAX).prop_filter_map("in range", EpiExtension::new)
}

/// Any User Defined field (133.1 §4.1.2.5, EPP-008).
pub fn user_defined() -> impl Strategy<Value = UserDefined> {
    (0..=UserDefined::MAX).prop_filter_map("in range", UserDefined::new)
}

/// Header fields with any EPI: EPI `110` with any extension, the others with none, and any
/// User Defined bits (133.1 §4.1.2; EPP-006, EPP-008).
pub fn encapsulation_fields() -> impl Strategy<Value = EncapsulationFields> {
    (epi(), epi_extension(), user_defined()).prop_map(|(epi, ext, user)| {
        if epi == Epi::EXTENDED {
            EncapsulationFields::extended(ext).with_user_defined(user)
        } else {
            EncapsulationFields::new(epi).with_user_defined(user)
        }
    })
}

/// Any header length (133.1 §4.1.2.4, Table 4-1; EPP-002).
pub fn header_len() -> impl Strategy<Value = HeaderLen> {
    prop_oneof![
        Just(HeaderLen::One),
        Just(HeaderLen::Two),
        Just(HeaderLen::Four),
        Just(HeaderLen::Eight),
    ]
}

/// The smallest header or a fixed one (133.1 §4.1.2.1.1, EPP-008).
pub fn header_size() -> impl Strategy<Value = HeaderSize> {
    prop_oneof![
        1 => Just(HeaderSize::Auto),
        4 => header_len().prop_map(HeaderSize::Fixed),
    ]
}

/// The managed parameters that accept every EPI, every extended EPI, and every data length,
/// for properties that are not about them (133.1 Table 5-1, EPP-010).
///
/// # Panics
/// Never: the parameters are within the limits of `EppConfig::new`.
#[must_use]
pub fn accept_all() -> EppConfig {
    EppConfig::new(
        0,
        MAX_DATA_LEN,
        EpiSet::from_bits(u8::MAX),
        ExtendedEpiSet::from_bits(u16::MAX),
    )
    .expect("a valid configuration")
}

/// Valid managed parameters: a length range within 0 to `MAX_DATA_LEN`, the edges included,
/// and any sets of valid EPIs and extended EPIs (133.1 Table 5-1; EPP-010, GEN-060).
pub fn epp_config() -> impl Strategy<Value = EppConfig> {
    (
        up_to_u32(MAX_DATA_LEN),
        up_to_u32(MAX_DATA_LEN),
        any::<u8>(),
        any::<u16>(),
    )
        .prop_filter_map("min <= max <= MAX_DATA_LEN", |(a, b, epis, ext)| {
            EppConfig::new(
                a.min(b),
                a.max(b),
                EpiSet::from_bits(epis),
                ExtendedEpiSet::from_bits(ext),
            )
            .ok()
        })
}

/// An Encapsulation Packet built by `EppConfig::data_unit` and `encode`, with the parts it was
/// built from (133.1 §4.1; EPP-002, EPP-008, API-020, API-021).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedEncapsulationPacket {
    /// The configuration the data unit was checked against.
    pub config: EppConfig,
    /// The header fields.
    pub fields: EncapsulationFields,
    /// The header size policy the packet was encoded with.
    pub size: HeaderSize,
    /// The data field.
    pub data: Vec<u8>,
    /// The encoded packet.
    pub octets: Vec<u8>,
}

impl EncodedEncapsulationPacket {
    /// The checked data unit, as `encode` takes it (EPP-010, RS-020).
    ///
    /// # Panics
    /// Never for a packet from these strategies, whose data unit was checked when it was built.
    #[must_use]
    pub fn data_unit(&self) -> DataUnit<'_> {
        self.config
            .data_unit(self.fields, &self.data)
            .expect("checked when built")
    }
}

/// A packet with `fields` and `0..max_data` data octets (133.1 §4.1.2; EPP-005, EPP-008).
///
/// The header size comes from [`header_size`] and the managed parameters are [`accept_all`].
/// Combinations the encoder rejects (a header too small for the fields or the length, or an
/// empty data field with a non-idle EPI) are not drawn.
pub fn encapsulation_packet_with(
    fields: impl Strategy<Value = EncapsulationFields>,
    max_data: usize,
) -> impl Strategy<Value = EncodedEncapsulationPacket> {
    (fields, header_size(), octets(0..max_data)).prop_filter_map(
        "an encodable packet",
        |(fields, size, data)| {
            let config = accept_all();
            let unit = config.data_unit(fields, &data).ok()?;
            let mut out = vec![0_u8; data.len() + 8];
            let n = encode(&unit, size, &mut out).ok()?;
            out.truncate(n);
            Some(EncodedEncapsulationPacket {
                config,
                fields,
                size,
                data,
                octets: out,
            })
        },
    )
}

/// A packet with any [`encapsulation_fields`] and up to 299 data octets, so that 2- and
/// 4-octet headers both occur ([`encapsulation_packet_with`]).
pub fn encapsulation_packet() -> impl Strategy<Value = EncodedEncapsulationPacket> {
    encapsulation_packet_with(encapsulation_fields(), 300)
}

/// An Encapsulation Idle Packet of 1–299 octets filled with any octet, encoded by `idle`
/// (133.1 §4.1.3.1.3; EPP-004, EPP-009).
pub fn idle_packet() -> impl Strategy<Value = Vec<u8>> {
    (1_usize..300, any::<u8>()).prop_filter_map("an encodable Idle Packet", |(len, fill)| {
        let mut out = vec![0_u8; len];
        let n = idle(len, fill, &mut out).ok()?;
        out.truncate(n);
        Some(out)
    })
}

// ----- Packet octets next to the decoder's boundaries -----

/// Octet 0 of the header (133.1 §4.1.2.2–4): the Packet Version Number in bits 0–2, the EPI in
/// bits 3–5, and the Length of Length in bits 6–7.
const VERSION_MASK: u8 = 0b1110_0000;
const EPI_MASK: u8 = 0b0001_1100;
/// The Protocol ID Extension, the low four bits of octet 1 of a 4- or 8-octet header (133.1
/// §4.1.2.6).
const EXTENSION_MASK: u8 = 0x0F;

/// The header length the Length of Length of `packet` gives (133.1 §4.1.2.4, Table 4-1).
const fn header_len_of(packet: &[u8]) -> HeaderLen {
    HeaderLen::from_length_of_length(packet[0])
}

/// The octets of the Packet Length field of a header of length `len` (133.1 §4.1.2.7, Figure
/// 4-2): octet 1 of a 2-octet header, octets 2–3 of a 4-octet header, octets 4–7 of an 8-octet
/// header; a 1-octet header has none.
const fn length_field(len: HeaderLen) -> core::ops::Range<usize> {
    match len {
        HeaderLen::One => 0..0,
        HeaderLen::Two => 1..2,
        HeaderLen::Four => 2..4,
        HeaderLen::Eight => 4..8,
    }
}

/// `packet` with its Packet Length changed by `delta`, wrapping within the field's width
/// (133.1 §4.1.2.7, EPP-002). A 1-octet header is returned unchanged.
fn with_length_delta(mut packet: Vec<u8>, delta: i32) -> Vec<u8> {
    let range = length_field(header_len_of(&packet));
    let field = &mut packet[range];
    let mut wide = [0_u8; 4];
    let start = wide.len() - field.len();
    wide[start..].copy_from_slice(field);
    let changed = u32::from_be_bytes(wide)
        .wrapping_add_signed(delta)
        .to_be_bytes();
    field.copy_from_slice(&changed[start..]);
    packet
}

/// Encapsulation Packet octets next to the boundaries of `EncapsulationPacket::decode` (133.1
/// §4.1; GEN-020, API-030, VER-023):
///
/// - a valid packet or Idle Packet, possibly followed by other octets;
/// - a proper prefix of one: one octet too short, or cut inside the header (EPP-002);
/// - the Packet Length one more or one less than the octets present (EPP-002, EPP-003);
/// - a Packet Version Number other than `111` (EPP-001);
/// - a non-zero Protocol ID Extension while the EPI is not `110` (EPP-006);
/// - a non-zero CCSDS Defined Field in an 8-octet header (EPP-007);
/// - EPI `110` in a 2-octet header, which has no extension (EPP-012);
/// - a non-idle EPI in a 1-octet header (EPP-004).
pub fn encapsulation_packet_near_boundary() -> BoxedStrategy<Vec<u8>> {
    let valid = prop_oneof![
        3 => encapsulation_packet().prop_map(|p| p.octets),
        1 => idle_packet(),
    ];
    let long_header = encapsulation_packet()
        .prop_filter_map("a 4- or 8-octet header", |p| {
            header_len_of(&p.octets).has_extension().then_some(p.octets)
        })
        .boxed();
    let non_extended = Epi::EXTENDED.get();
    prop_oneof![
        valid.clone().prop_flat_map(|v| with_trailing(v, 4)).boxed(),
        valid.clone().prop_flat_map(proper_prefix).boxed(),
        (valid.clone(), prop_oneof![Just(1), Just(-1)])
            .prop_map(|(v, d)| with_length_delta(v, d))
            .boxed(),
        (
            valid,
            packet_version_other_than(PacketVersion::ENCAPSULATION)
        )
            .prop_map(|(mut v, version)| {
                v[0] = (v[0] & !VERSION_MASK) | (version.get() << 5);
                v
            })
            .boxed(),
        (long_header.clone(), 1_u8..=EXTENSION_MASK)
            .prop_filter_map("EPI other than 110", move |(mut v, ext)| {
                ((v[0] & EPI_MASK) >> 2 != non_extended).then(|| {
                    v[1] |= ext;
                    v
                })
            })
            .boxed(),
        (long_header, 1_u16..)
            .prop_filter_map("an 8-octet header", |(mut v, ccsds)| {
                (header_len_of(&v) == HeaderLen::Eight).then(|| {
                    v[2..4].copy_from_slice(&ccsds.to_be_bytes());
                    v
                })
            })
            .boxed(),
        encapsulation_packet()
            .prop_filter_map("a 2-octet header", move |p| {
                let mut v = p.octets;
                (header_len_of(&v) == HeaderLen::Two).then(|| {
                    v[0] = (v[0] & !EPI_MASK) | (non_extended << 2);
                    v
                })
            })
            .boxed(),
        (1..=Epi::MAX)
            .prop_map(|epi| vec![(PacketVersion::ENCAPSULATION.get() << 5) | (epi << 2)])
            .boxed(),
    ]
    .boxed()
}

/// Input for an Encapsulation Packet decoder property: arbitrary octets or octets
/// [near a boundary](encapsulation_packet_near_boundary) (GEN-020, API-030, VER-023).
pub fn encapsulation_packet_decoder_input() -> BoxedStrategy<Vec<u8>> {
    prop_oneof![1 => octets(0..64), 3 => encapsulation_packet_near_boundary()].boxed()
}

#[cfg(test)]
mod tests {
    use super::{
        encapsulation_packet, encapsulation_packet_decoder_input,
        encapsulation_packet_near_boundary, epp_config, idle_packet, with_length_delta,
    };
    use crate::strategies::config;
    use aldis_core::{DecodeMode, ErrorKind};
    use aldis_epp::config::MAX_DATA_LEN;
    use aldis_epp::{EncapsulationPacket, HeaderLen};
    use proptest::prelude::*;

    const REGRESSIONS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/proptest-regressions/strategies.txt"
    );

    proptest! {
        #![proptest_config(config(256, REGRESSIONS))]

        #[test]
        fn packets_decode_strictly(p in encapsulation_packet(), idle in idle_packet()) {
            // req: VER-054, EPP-002, EPP-009
            let d = EncapsulationPacket::decode(&p.octets, DecodeMode::Strict).unwrap();
            prop_assert_eq!(d.epi(), p.fields.epi());
            prop_assert_eq!(d.data(), p.data.as_slice());
            prop_assert_eq!(p.data_unit().data(), p.data.as_slice());
            prop_assert!(EncapsulationPacket::decode(&idle, DecodeMode::Strict).unwrap().is_idle());
        }

        #[test]
        fn longer_length_is_incomplete(p in encapsulation_packet()) {
            // req: VER-054, EPP-002
            let len = HeaderLen::from_length_of_length(p.octets[0]);
            prop_assume!(len != HeaderLen::One);
            prop_assume!(u32::try_from(p.octets.len()).unwrap() < len.max_packet_len());
            let longer = with_length_delta(p.octets, 1);
            prop_assert_eq!(
                EncapsulationPacket::decode(&longer, DecodeMode::Strict).unwrap_err().kind(),
                ErrorKind::Incomplete
            );
        }

        #[test]
        fn other_strategies_are_in_range(
            c in epp_config(),
            a in encapsulation_packet_near_boundary(),
            b in encapsulation_packet_decoder_input(),
        ) {
            // req: VER-054, GEN-020
            prop_assert!(c.min_data_len() <= c.max_data_len() && c.max_data_len() <= MAX_DATA_LEN);
            for bytes in [a, b] {
                let _ = EncapsulationPacket::decode(&bytes, DecodeMode::Strict);
                let _ = EncapsulationPacket::decode(&bytes, DecodeMode::Lenient);
            }
        }
    }
}
