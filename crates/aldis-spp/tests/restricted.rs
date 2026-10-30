//! A test for every restricted type of `aldis-spp` (M2 plan WP-7; RS-020, VER-023): every
//! public type with a checked constructor, as `cargo xtask restricted` lists them. Each test
//! names its type with a `// restricted:` marker and shows that the constructor accepts exactly
//! the range the Blue Book clause gives, that every rejected value is rejected with the
//! documented error, and that every accepted value works on the wire: a packet built under it
//! encodes and decodes. The spec bounds are written from the clause, never from the crate's
//! constants, which are compared with them. Generated inputs come from the shared strategies of
//! `aldis-testkit` (VER-054).
#![expect(
    clippy::arithmetic_side_effects,
    reason = "test harness: lengths are small and bounded (RS-018)"
)]

use aldis_core::{Error, ErrorDetail, ErrorKind};
use aldis_spp::{MAX_PACKET_LEN, MIN_PACKET_LEN, SpacePacket, SppConfig, encode};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::spp::{max_packet_len_out_of_range, spp_config, user_packet_fields};
use aldis_testkit::strategies::{self};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/restricted.txt"
);

/// A Space Packet has at least 7 and at most 65 542 octets (133.0 §4.1.2.2).
const SPEC_MIN_PACKET_LEN: usize = 7;
/// See [`SPEC_MIN_PACKET_LEN`].
const SPEC_MAX_PACKET_LEN: usize = 65_542;

/// `SppConfig::new` accepts exactly the packet lengths of 133.0 §4.1.2.2, every length 0–70 000
/// and `usize::MAX`, and rejects the others with `InvalidConfig` (`max_packet_len`).
#[test]
fn spp_config_accepts_exactly_the_packet_lengths() {
    // restricted: SppConfig
    // req: SPP-006, GEN-060, RS-020, VER-023
    assert_eq!(
        (MIN_PACKET_LEN, MAX_PACKET_LEN),
        (SPEC_MIN_PACKET_LEN, SPEC_MAX_PACKET_LEN)
    );
    assert_eq!(
        SppConfig::DEFAULT.max_packet_len(),
        SPEC_MAX_PACKET_LEN,
        "the default allows the longest packet"
    );
    let rejected =
        Error::new(ErrorKind::InvalidConfig).with_field(aldis_spp::field::MAX_PACKET_LEN);
    for len in (0..=70_000).chain([usize::MAX]) {
        let valid = (SPEC_MIN_PACKET_LEN..=SPEC_MAX_PACKET_LEN).contains(&len);
        match SppConfig::new(len) {
            Ok(c) => {
                assert!(valid, "{len} accepted");
                assert_eq!(c.max_packet_len(), len);
            }
            Err(e) => {
                assert!(!valid, "{len} rejected");
                assert_eq!(e, rejected);
            }
        }
    }
}

proptest! {
    #![proptest_config(strategies::config(64, REGRESSIONS))]

    /// Under any accepted configuration, a packet of exactly the Maximum Packet Length encodes
    /// and decodes to that length, and a packet one octet longer is refused with `OutOfRange`
    /// (`packet_len`) (133.0 §4.1.2.2, Table 5-1; SPP-006). Lengths outside the range are
    /// rejected as documented.
    #[test]
    fn spp_config_limits_the_packets_on_the_wire(
        config in spp_config(),
        fields in user_packet_fields(),
        bad in max_packet_len_out_of_range(),
    ) {
        // restricted: SppConfig
        // req: SPP-006, GEN-060, API-020, VER-023, VER-054
        let max = config.max_packet_len();
        let user = vec![0x5A_u8; max - 6];
        let data = config.packet(fields, &[], &user).unwrap();
        let mut out = vec![0_u8; max];
        prop_assert_eq!(encode(&data, &mut out), Ok(max));
        let p = SpacePacket::decode(&out).unwrap();
        prop_assert_eq!(p.len(), max);
        prop_assert_eq!(p.data_field(), user.as_slice());
        let longer = vec![0x5A_u8; max - 5];
        let too_long = Error::out_of_range(
            aldis_spp::field::PACKET_LEN,
            u64::try_from(max).unwrap(),
            u64::try_from(max + 1).unwrap(),
        );
        prop_assert_eq!(config.packet(fields, &[], &longer).err(), Some(too_long));
        prop_assert_eq!(
            SppConfig::new(bad),
            Err(Error::new(ErrorKind::InvalidConfig).with_field(aldis_spp::field::MAX_PACKET_LEN))
        );
        prop_assert_eq!(too_long.detail(), ErrorDetail::Expected {
            expected: u64::try_from(max).unwrap(),
            found: u64::try_from(max + 1).unwrap(),
        });
    }
}

/// The time secondary header (feature `time`): 133.0 §4.1.4.2.2.2 allows only the CCSDS
/// segmented or unsegmented binary time codes of 301.0, that is CUC and CDS.
#[cfg(feature = "time")]
#[expect(
    clippy::unwrap_used,
    reason = "test fixtures built from valid formats; a failure must be loud (RS-018)"
)]
mod time {
    use aldis_core::{Error, ErrorKind};
    use aldis_spp::{
        PacketFields, SecondaryHeaderLayout, SpacePacket, SppConfig, TimeHeaderLayout, encode,
    };
    use aldis_testkit::strategies::proptest::prelude::*;
    use aldis_testkit::strategies::spp::user_packet_fields;
    use aldis_testkit::strategies::{self, octets};
    use aldis_time::{
        AgencyFormat, CcsFormat, CcsVariation, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CucEpoch,
        CucFormat, TimeFormat,
    };

    use super::REGRESSIONS;

    /// Every time code format with its default P-field: each CUC format (1–7 coarse, 0–10 fine
    /// octets, both epochs), each CDS format, each CCS format (0–6 sub-second octets, both
    /// variations), and each agency-defined format (T-field of 1–16 octets).
    fn every_format() -> Vec<TimeFormat> {
        let mut out = Vec::new();
        for epoch in [CucEpoch::Tai1958, CucEpoch::Agency] {
            for coarse in 0..=u8::MAX {
                for fine in 0..=u8::MAX {
                    if let Ok(f) = CucFormat::new(epoch, coarse, fine) {
                        out.push(TimeFormat::Cuc(f));
                    }
                }
            }
        }
        for epoch in [CdsEpoch::Ccsds1958, CdsEpoch::Agency] {
            for day in [CdsDayLen::Bits16, CdsDayLen::Bits24] {
                for subms in [CdsSubMs::None, CdsSubMs::Micros, CdsSubMs::Picos] {
                    out.push(TimeFormat::Cds(CdsFormat::new(epoch, day, subms)));
                }
            }
        }
        for variation in [CcsVariation::MonthDay, CcsVariation::DayOfYear] {
            for sub in 0..=u8::MAX {
                if let Ok(f) = CcsFormat::new(variation, sub) {
                    out.push(TimeFormat::Ccs(f));
                }
            }
        }
        for t_len in 0..=u8::MAX {
            if let Ok(f) = AgencyFormat::new(t_len) {
                out.push(TimeFormat::Agency(f));
            }
        }
        out
    }

    /// Whether 133.0 §4.1.4.2.2.2 allows `format` in a Packet Secondary Header.
    const fn allowed(format: &TimeFormat) -> bool {
        matches!(format, TimeFormat::Cuc(_) | TimeFormat::Cds(_))
    }

    /// `TimeHeaderLayout::new` accepts exactly the CUC and CDS formats, with or without the
    /// P-field and with any ancillary length, and rejects CCS and agency-defined codes with
    /// `InvalidConfig` (`time_code`) (133.0 §4.1.4.2.2.2, OI-30).
    #[test]
    fn time_header_layout_accepts_exactly_cuc_and_cds() {
        // restricted: TimeHeaderLayout
        // req: SPP-008, RS-020, VER-023
        let formats = every_format();
        assert_eq!(formats.len(), 2 * 7 * 11 + 2 * 2 * 3 + 2 * 7 + 16);
        let rejected = Error::new(ErrorKind::InvalidConfig).with_field(aldis_spp::field::TIME_CODE);
        for format in formats {
            for pfield in [false, true] {
                for ancillary in [0, 1, usize::MAX] {
                    match TimeHeaderLayout::new(format, pfield, ancillary) {
                        Ok(l) => {
                            assert!(allowed(&format), "{format:?} accepted");
                            assert_eq!(l.format(), format);
                            assert_eq!(l.has_pfield(), pfield);
                            assert_eq!(l.ancillary_len(), ancillary);
                        }
                        Err(e) => {
                            assert!(!allowed(&format), "{format:?} rejected");
                            assert_eq!(e, rejected);
                        }
                    }
                }
            }
        }
    }

    /// A packet whose secondary header is a time code of `format` (all-zero T-field, which every
    /// CUC and CDS format allows), with its P-field if `pfield`, then `ancillary`.
    fn packet(
        fields: PacketFields,
        format: &TimeFormat,
        pfield: bool,
        ancillary: &[u8],
    ) -> (Vec<u8>, Vec<u8>) {
        let p = format.pfield();
        let mut code = Vec::new();
        if pfield {
            code.push(p.first());
            code.extend_from_slice(p.rest());
        }
        code.resize(code.len() + format.t_len(), 0);
        let secondary = [code.as_slice(), ancillary].concat();
        let data = SppConfig::DEFAULT
            .packet(fields, &secondary, &[0xC3])
            .unwrap();
        let mut out = vec![0_u8; data.packet_len()];
        encode(&data, &mut out).unwrap();
        (out, code)
    }

    proptest! {
        #![proptest_config(strategies::config(128, REGRESSIONS))]

        /// For every accepted layout, a packet whose secondary header is a time code of the
        /// layout's format and its ancillary data decodes and splits into the same time code,
        /// which re-encodes to the same octets, and the same ancillary data (133.0
        /// §4.1.4.2.2–3; SPP-007, SPP-008, API-020).
        #[test]
        fn time_header_layouts_split_their_packets(
            format in prop::sample::select(every_format()).prop_filter("allowed", allowed),
            pfield in any::<bool>(),
            ancillary in octets(0..6),
            fields in user_packet_fields(),
        ) {
            // restricted: TimeHeaderLayout
            // req: SPP-007, SPP-008, API-020, VER-023, VER-054
            let layout = TimeHeaderLayout::new(format, pfield, ancillary.len()).unwrap();
            let (octets, code) = packet(fields, &format, pfield, &ancillary);
            let p = SpacePacket::decode(&octets).unwrap();
            let s = p.split(&SecondaryHeaderLayout::Time(layout)).unwrap();
            let t = s.time_code().unwrap();
            prop_assert_eq!(t.len(), code.len());
            prop_assert_eq!(t.pfield().is_some(), pfield);
            let mut again = [0_u8; 32];
            let n = t.code().encode(pfield, &mut again).unwrap();
            prop_assert_eq!(again.get(..n), Some(code.as_slice()));
            prop_assert_eq!(s.ancillary_data(), ancillary.as_slice());
            prop_assert_eq!(s.user_data(), &[0xC3][..]);
        }
    }
}
