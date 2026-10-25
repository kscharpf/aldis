//! A test for every restricted type of `aldis-epp` (M2 plan WP-7; RS-020, VER-023): every
//! public type with a checked constructor, as `cargo xtask restricted` lists them. Each test
//! names its type with a `// restricted:` marker and shows that the constructor accepts exactly
//! the range the Blue Book clause gives, over the whole input domain where it is small, that
//! every rejected value is rejected with the documented error, and that every accepted value goes
//! to its header field and back unchanged. The spec bounds are written from the clause, never
//! from the crate's constants, which are compared with them. Generated inputs come from the
//! shared strategies of `aldis-testkit` (VER-054).
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test harness: lengths are small and bounded, a failure must be loud (RS-018)"
)]

use aldis_core::{DecodeMode, Error, ErrorDetail, ErrorKind, FieldId};
use aldis_epp::config::MAX_DATA_LEN;
use aldis_epp::{
    EncapsulationFields, EncapsulationHeader, EncapsulationPacket, Epi, EpiExtension, EpiSet,
    EppConfig, ExtendedEpiSet, HeaderLen, HeaderSize, UserDefined, encode, field, idle,
};
use aldis_testkit::strategies::common::above_u8;
use aldis_testkit::strategies::epp::{
    accept_all, epi, epi_extension, epp_config_bounds, user_defined,
};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::{self, octets};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/restricted.txt"
);

/// The largest value of a field of `bits` bits, from the field width a clause gives.
const fn field_max(bits: u32) -> u8 {
    u8::MAX >> (8 - bits)
}

/// The `OutOfRange` error a range-checked identifier documents (API-001, GEN-041).
fn out_of_range(field: FieldId, max: u8, found: u8) -> Error {
    Error::out_of_range(field, u64::from(max), u64::from(found))
}

/// The identifiers of a packet decoded in strict mode.
type Ids = (Epi, Option<EpiExtension>, Option<UserDefined>);

/// Encodes one data octet with `fields` under a fixed `len` header, decodes it again, and
/// returns the decoded identifiers: the fields' wire form (133.1 §4.1.2, API-021).
fn through_header(fields: EncapsulationFields, len: HeaderLen) -> Ids {
    let unit = accept_all().data_unit(fields, &[0x3C]).unwrap();
    let mut out = [0_u8; 9];
    let n = encode(&unit, HeaderSize::Fixed(len), &mut out).unwrap();
    let p = EncapsulationPacket::decode(out.get(..n).unwrap(), DecodeMode::Strict).unwrap();
    assert_eq!((p.header_len(), p.data()), (len, &[0x3C_u8][..]));
    (p.epi(), p.extension(), p.user_defined())
}

/// The Encapsulation Protocol ID is bits 3–5 of the Packet Header, 3 bits (133.1 §4.1.2.3.1);
/// `000` is idle, `110` extended, `111` mission-specific (§4.1.2.3, notes 1–3). Every `u8`.
#[test]
fn epi_accepts_exactly_3_bits() {
    // restricted: Epi
    // req: API-001, EPP-008, EPP-011, VER-023
    let spec_max = field_max(3);
    assert_eq!(Epi::MAX, spec_max);
    assert_eq!(
        (
            Epi::IDLE.get(),
            Epi::EXTENDED.get(),
            Epi::MISSION_SPECIFIC.get()
        ),
        (0b000, 0b110, 0b111)
    );
    for v in 0..=u8::MAX {
        if let Some(e) = Epi::new(v) {
            assert!(v <= spec_max, "{v} accepted");
            assert_eq!((e.get(), u8::from(e)), (v, v));
            assert_eq!(Epi::try_from(v), Ok(e));
            // EPI 110 needs a 4- or 8-octet header (EPP-012); every EPI fits a 4-octet one.
            let (got, _, _) = through_header(EncapsulationFields::new(e), HeaderLen::Four);
            assert_eq!(got, e, "EPI {v} through its field");
        } else {
            assert!(v > spec_max, "{v} rejected");
            assert_eq!(Epi::try_from(v), Err(out_of_range(field::EPI, spec_max, v)));
        }
    }
}

/// The Encapsulation Protocol ID Extension is four bits (133.1 §4.1.2.6.1) and identifies the
/// protocol with EPI `110` (§4.1.2.6.2). Every `u8`.
#[test]
fn epi_extension_accepts_exactly_4_bits() {
    // restricted: EpiExtension
    // req: API-001, EPP-006, EPP-008, EPP-011, VER-023
    let spec_max = field_max(4);
    assert_eq!(EpiExtension::MAX, spec_max);
    for v in 0..=u8::MAX {
        if let Some(x) = EpiExtension::new(v) {
            assert!(v <= spec_max, "{v} accepted");
            assert_eq!((x.get(), u8::from(x)), (v, v));
            assert_eq!(EpiExtension::try_from(v), Ok(x));
            for len in [HeaderLen::Four, HeaderLen::Eight] {
                let (_, got, _) = through_header(EncapsulationFields::extended(x), len);
                assert_eq!(got, Some(x), "extension {v} through its field");
            }
        } else {
            assert!(v > spec_max, "{v} rejected");
            assert_eq!(
                EpiExtension::try_from(v),
                Err(out_of_range(field::EPI_EXTENSION, spec_max, v))
            );
        }
    }
}

/// The User Defined field is four bits (133.1 §4.1.2.5.1). Every `u8`.
#[test]
fn user_defined_accepts_exactly_4_bits() {
    // restricted: UserDefined
    // req: API-001, EPP-008, EPP-011, VER-023
    let spec_max = field_max(4);
    assert_eq!(UserDefined::MAX, spec_max);
    for v in 0..=u8::MAX {
        if let Some(u) = UserDefined::new(v) {
            assert!(v <= spec_max, "{v} accepted");
            assert_eq!((u.get(), u8::from(u)), (v, v));
            assert_eq!(UserDefined::try_from(v), Ok(u));
            for len in [HeaderLen::Four, HeaderLen::Eight] {
                let f = EncapsulationFields::new(Epi::MISSION_SPECIFIC).with_user_defined(u);
                let (_, _, got) = through_header(f, len);
                assert_eq!(got, Some(u), "user defined {v} through its field");
            }
        } else {
            assert!(v > spec_max, "{v} rejected");
            assert_eq!(
                UserDefined::try_from(v),
                Err(out_of_range(field::USER_DEFINED, spec_max, v))
            );
        }
    }
}

/// `EncapsulationFields::from_parts` accepts exactly the combinations of 133.1 §4.1.2.6.3: the
/// Protocol ID Extension is all zeros unless the EPI is `110`. Every combination of EPI,
/// extension, and User Defined bits; each accepted one goes through a 4- and an 8-octet header
/// unchanged, and each rejected one is `InvalidField` (`epi_extension`).
#[test]
fn encapsulation_fields_accept_exactly_the_extension_rule() {
    // restricted: EncapsulationFields
    // req: EPP-006, EPP-008, RS-020, VER-023
    let extended = Epi::new(0b110).unwrap();
    let rejected = Error::new(ErrorKind::InvalidField).with_field(field::EPI_EXTENSION);
    for e in (0..=Epi::MAX).filter_map(Epi::new) {
        for x in (0..=EpiExtension::MAX).filter_map(EpiExtension::new) {
            for u in (0..=UserDefined::MAX).filter_map(UserDefined::new) {
                let valid = x.get() == 0 || e == extended;
                match EncapsulationFields::from_parts(e, x, u) {
                    Ok(f) => {
                        assert!(valid, "{e:?} {x:?} accepted");
                        assert_eq!((f.epi(), f.extension(), f.user_defined()), (e, x, u));
                        for len in [HeaderLen::Four, HeaderLen::Eight] {
                            assert_eq!(through_header(f, len), (e, Some(x), Some(u)));
                        }
                    }
                    Err(err) => {
                        assert!(!valid, "{e:?} {x:?} rejected");
                        assert_eq!(err, rejected);
                    }
                }
            }
        }
    }
}

/// The header an Idle Packet of `total` octets needs: the smallest of 133.1 Table 4-2 whose
/// packet length range holds `total` (1; 2–255; up to 65 535; up to 4 294 967 295 octets).
const fn spec_idle_header(total: u64) -> Option<HeaderLen> {
    match total {
        1 => Some(HeaderLen::One),
        2..=255 => Some(HeaderLen::Two),
        256..=65_535 => Some(HeaderLen::Four),
        65_536..=4_294_967_295 => Some(HeaderLen::Eight),
        _ => None,
    }
}

/// `EncapsulationHeader::idle` accepts exactly the packet lengths of 133.1 Table 4-2, 1 to
/// 4 294 967 295 octets, with the smallest header that fits (§4.1.3.1.3, EPP-009), and rejects
/// 0 and longer lengths with `OutOfRange` (`packet_len`) naming the violated bound. Every length
/// 0–70 000 and the edges of the 8-octet range; each accepted header encodes to a header whose
/// Packet Length gives the same total length, and lengths up to 300 decode as Idle Packets.
#[test]
fn idle_header_accepts_exactly_the_packet_lengths() {
    // restricted: EncapsulationHeader
    // req: EPP-002, EPP-009, RS-020, VER-023
    let edges = [0xFFFF_FFFE_u64, 0xFFFF_FFFF, 0x1_0000_0000]
        .into_iter()
        .filter_map(|n| usize::try_from(n).ok())
        .chain([usize::MAX - 1, usize::MAX]);
    for total in (0..=70_000).chain(edges) {
        let wide = u64::try_from(total).unwrap();
        match (EncapsulationHeader::idle(total), spec_idle_header(wide)) {
            (Ok(h), Some(len)) => {
                assert_eq!(h.header_len(), len, "{total} octets");
                assert_eq!(u64::from(h.packet_len()), wide);
                assert!(h.epi().is_idle());
                let mut out = [0_u8; 8];
                let n = h.encode(&mut out).unwrap();
                assert_eq!(n, len.octets());
                if n == 1 {
                    assert_eq!(total, 1);
                } else {
                    let header = out.get(..n).unwrap();
                    assert_eq!(EncapsulationPacket::peek_length(header), Ok(total));
                }
                if total <= 300 {
                    let mut packet = vec![0_u8; total];
                    assert_eq!(idle(total, 0x55, &mut packet), Ok(total));
                    let p = EncapsulationPacket::decode(&packet, DecodeMode::Strict).unwrap();
                    assert!(p.is_idle());
                    assert_eq!((p.len(), p.header_len()), (total, len));
                }
            }
            (Err(e), None) => {
                let limit = if total == 0 { 1 } else { u64::from(u32::MAX) };
                assert_eq!(e, Error::out_of_range(field::PACKET_LEN, limit, wide));
            }
            (got, want) => panic!("{total} octets: {got:?}, Table 4-2 {want:?}"),
        }
    }
}

/// The largest Encapsulated Data Field, 4 294 967 287 octets (133.1 Table 4-2, 8-octet header).
const SPEC_MAX_DATA_LEN: u32 = 4_294_967_287;

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    /// `EppConfig::new` accepts exactly a minimum at most the maximum, and a maximum at most the
    /// largest Encapsulated Data Field of 133.1 Table 4-2 (Table 5-1; EPP-010, GEN-060). The
    /// order of the bounds is checked first; each rejection is `InvalidConfig` naming the bound
    /// with the documented detail.
    #[test]
    fn epp_config_accepts_exactly_the_data_lengths((min, max) in epp_config_bounds()) {
        // restricted: EppConfig
        // req: EPP-010, GEN-060, RS-020, VER-023, VER-054
        prop_assert_eq!(MAX_DATA_LEN, SPEC_MAX_DATA_LEN);
        let sets = (EpiSet::from_bits(0xFF), ExtendedEpiSet::from_bits(0xFFFF));
        let got = EppConfig::new(min, max, sets.0, sets.1);
        let config = |field: FieldId, expected: u32, found: u32| {
            Error::new(ErrorKind::InvalidConfig)
                .with_field(field)
                .with_detail(ErrorDetail::Expected {
                    expected: expected.into(),
                    found: found.into(),
                })
        };
        if min > max {
            prop_assert_eq!(got, Err(config(field::MIN_DATA_LEN, max, min)));
        } else if max > SPEC_MAX_DATA_LEN {
            prop_assert_eq!(got, Err(config(field::MAX_DATA_LEN, SPEC_MAX_DATA_LEN, max)));
        } else {
            let c = got.unwrap();
            prop_assert_eq!((c.min_data_len(), c.max_data_len()), (min, max));
            prop_assert_eq!((c.valid_epis(), c.valid_extended_epis()), sets);
        }
    }

    /// Under an accepted configuration, a data unit whose length lies in its range and whose
    /// EPI is valid encodes, decodes, and passes `validate`; one octet below the minimum or above
    /// the maximum is refused with `OutOfRange` (`data_len`) (133.1 §4.2.1.1.2; EPP-010).
    #[test]
    fn epp_config_limits_the_packets_on_the_wire(
        a in 1_u32..200,
        b in 1_u32..200,
        e in epi().prop_filter("not idle", |e| !e.is_idle()),
        x in epi_extension(),
        u in user_defined(),
        data in octets(0..201),
    ) {
        // restricted: EppConfig
        // req: EPP-010, GEN-060, API-020, VER-023, VER-054
        let (min, max) = (a.min(b), a.max(b));
        let set = EpiSet::EMPTY.with(e);
        let config = EppConfig::new(min, max, set, ExtendedEpiSet::EMPTY.with(x)).unwrap();
        let fields = if e == Epi::EXTENDED {
            EncapsulationFields::extended(x)
        } else {
            EncapsulationFields::new(e)
        }
        .with_user_defined(u);
        let len = u32::try_from(data.len()).unwrap();
        match config.data_unit(fields, &data) {
            Ok(unit) => {
                prop_assert!((min..=max).contains(&len));
                let mut out = vec![0_u8; data.len() + 8];
                let n = encode(&unit, HeaderSize::Auto, &mut out).unwrap();
                let p = EncapsulationPacket::decode(out.get(..n).unwrap(), DecodeMode::Strict)
                    .unwrap();
                prop_assert_eq!(p.data(), data.as_slice());
                prop_assert_eq!(p.validate(&config), Ok(()));
            }
            Err(err) => {
                prop_assert!(!(min..=max).contains(&len));
                let bound = if len < min { min } else { max };
                prop_assert_eq!(
                    err,
                    Error::out_of_range(field::DATA_LEN, bound.into(), len.into())
                );
            }
        }
    }

    /// The identifiers drawn by the shared strategies go through their header fields unchanged,
    /// and the values above them are rejected with `OutOfRange` (133.1 §4.1.2.3, §4.1.2.5–6;
    /// API-001).
    #[test]
    fn identifiers_from_the_strategies_round_trip(
        e in epi(),
        x in epi_extension(),
        u in user_defined(),
        bad_epi in above_u8(Epi::MAX),
        bad_ext in above_u8(EpiExtension::MAX),
        bad_user in above_u8(UserDefined::MAX),
    ) {
        // restricted: Epi, EpiExtension, UserDefined
        // req: API-001, EPP-008, EPP-011, VER-023, VER-054
        let (fields, ext) = if e == Epi::EXTENDED {
            (EncapsulationFields::extended(x), x)
        } else {
            (EncapsulationFields::new(e), EpiExtension::default())
        };
        let got = through_header(fields.with_user_defined(u), HeaderLen::Eight);
        prop_assert_eq!(got, (e, Some(ext), Some(u)));
        prop_assert_eq!(Epi::try_from(bad_epi), Err(out_of_range(field::EPI, field_max(3), bad_epi)));
        prop_assert_eq!(
            EpiExtension::try_from(bad_ext),
            Err(out_of_range(field::EPI_EXTENSION, field_max(4), bad_ext))
        );
        prop_assert_eq!(
            UserDefined::try_from(bad_user),
            Err(out_of_range(field::USER_DEFINED, field_max(4), bad_user))
        );
    }
}
