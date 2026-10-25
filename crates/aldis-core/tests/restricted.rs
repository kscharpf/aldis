//! A test for every restricted type of `aldis-core` (M2 plan WP-7; RS-020, VER-023): every
//! public type with a checked constructor, as `cargo xtask restricted` lists them. Each test
//! names its type with a `// restricted:` marker and shows that
//!
//! - the constructor accepts exactly the range the Blue Book clause (or the requirement) gives,
//!   over the whole input domain where it is at most 16 bits wide, and at its boundaries plus
//!   generated values otherwise;
//! - every rejected value is rejected with the documented error;
//! - every accepted value goes to its wire field and back unchanged, where the type has one.
//!
//! The spec bounds are written from the field width that the cited clause gives, never from
//! the type's own constants, which are compared with them. Generated inputs come from the shared
//! strategies of `aldis-testkit` (VER-054). An integration test, so that the strategies and the
//! tests see the same copy of the crate (M2 plan D6).
#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test harness: bounds and fixtures are small and fixed, a failure must be loud (RS-018)"
)]

use std::collections::BTreeMap;

use aldis_core::bits::{get, set};
use aldis_core::counter::ModCounter;
use aldis_core::error::{Error, ErrorDetail, ErrorKind, FieldId};
use aldis_core::field;
use aldis_core::ids::{Apid, MapId, PacketVersion, SeqCount, SpacecraftId, VcId};
use aldis_core::layout::FrameLayout;
use aldis_core::ocf::{FsrReport, Ocf, ProjectSpecificReport, Type1Report};
use aldis_core::security::{SecuredFields, SecurityLayout};
use aldis_testkit::strategies::common::{
    apid, apid_out_of_range, layout_spec, map_id, mod_counter, mod_counter_out_of_range,
    ocf_octets, packet_version, packet_version_out_of_range, secured_ranges, seq_count,
    seq_count_out_of_range, spacecraft_id, vc_id,
};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::{self};
use aldis_testkit::vectors_dir;

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/restricted.txt"
);

/// The largest value of a field of `bits` bits, from the field width a clause gives.
const fn field_max(bits: u32) -> u64 {
    (1_u64 << bits) - 1
}

/// The `OutOfRange` error a range-checked identifier documents for `found` (API-001, GEN-041).
const fn out_of_range(field: FieldId, max: u64, found: u64) -> Error {
    Error::new(ErrorKind::OutOfRange)
        .with_field(field)
        .with_detail(ErrorDetail::Expected {
            expected: max,
            found,
        })
}

/// Writes `value` into a `bits`-wide field at bit `offset` of a buffer of `len` octets and reads
/// it back: the field's wire form (GEN-050, GEN-051).
fn through_field(len: usize, offset: usize, bits: u8, value: u64) -> u64 {
    let mut buf = vec![0xA5_u8; len];
    set(&mut buf, offset, bits, value).unwrap();
    get(&buf, offset, bits).unwrap()
}

/// Where an identifier lies in its header: the header's length in octets, the field's first bit,
/// and its width in bits, from the clause.
type Position = (usize, usize, u8);

/// Checks an identifier over every raw value of type `R` (`u8` or `u16`): `new` accepts exactly
/// `0..=spec_max`, `TryFrom` rejects the rest with `OutOfRange` naming `field` with the type's
/// `max` and the value found, and every accepted value round-trips through its field at `at`.
fn check_id<R, T>(
    spec_max: u64,
    max: R,
    field: FieldId,
    (new, try_from, value): (
        impl Fn(R) -> Option<T>,
        impl Fn(R) -> Result<T, Error>,
        impl Fn(T) -> R,
    ),
    (len, offset, bits): Position,
) where
    R: Copy + Into<u64> + TryFrom<u64> + PartialEq + core::fmt::Debug + Raw,
    T: Copy + PartialEq + core::fmt::Debug,
{
    assert_eq!(max.into(), spec_max, "the type's MAX is the clause's");
    for v in R::all() {
        let wide: u64 = v.into();
        if let Some(id) = new(v) {
            assert!(wide <= spec_max, "{v:?} accepted above {spec_max}");
            assert_eq!(value(id), v);
            assert_eq!(try_from(v), Ok(id));
            let back = R::try_from(through_field(len, offset, bits, wide))
                .ok()
                .unwrap();
            assert_eq!(new(back), Some(id), "{v:?} through its field");
        } else {
            assert!(wide > spec_max, "{v:?} rejected within {spec_max}");
            assert_eq!(try_from(v), Err(out_of_range(field, max.into(), wide)));
        }
    }
}

/// A raw identifier type, `u8` or `u16`, with every value of it.
trait Raw: Sized {
    /// Every value, in order.
    fn all() -> impl Iterator<Item = Self>;
}

impl Raw for u8 {
    fn all() -> impl Iterator<Item = u8> {
        0..=u8::MAX
    }
}

impl Raw for u16 {
    fn all() -> impl Iterator<Item = u16> {
        0..=u16::MAX
    }
}

/// The Application Process Identifier is bits 5–15 of the Packet Primary Header, 11 bits, so
/// 0–2047 (133.0 §4.1.3.3.4.1); 2047 is the idle APID (§4.1.3.3.4.4).
#[test]
fn apid_accepts_exactly_11_bits() {
    // restricted: Apid
    // req: API-001, RS-010, SPP-002, VER-023
    check_id(
        field_max(11),
        Apid::MAX,
        field::APID,
        (Apid::new, Apid::try_from, Apid::get),
        (6, 5, 11),
    );
    assert_eq!(u64::from(Apid::IDLE.get()), field_max(11));
}

/// The Packet Sequence Count is bits 18–31 of the Packet Primary Header, 14 bits, so 0–16383
/// (133.0 §4.1.3.4.3.1).
#[test]
fn seq_count_accepts_exactly_14_bits() {
    // restricted: SeqCount
    // req: API-001, RS-010, SPP-011, VER-023
    check_id(
        field_max(14),
        SeqCount::MAX,
        field::SEQUENCE_COUNT,
        (SeqCount::new, SeqCount::try_from, SeqCount::get),
        (6, 18, 14),
    );
}

/// The Packet Version Number is bits 0–2 of a packet header, 3 bits (133.0 §4.1.3.2.1,
/// 133.1 §4.1.2.2.1); `000` is the Space Packet (133.0 §4.1.3.2.2), `111` the Encapsulation
/// Packet (133.1 §4.1.2.2.2).
#[test]
fn packet_version_accepts_exactly_3_bits() {
    // restricted: PacketVersion
    // req: API-001, RS-010, SPP-001, EPP-001, VER-023
    check_id(
        field_max(3),
        PacketVersion::MAX,
        field::PACKET_VERSION,
        (
            PacketVersion::new,
            PacketVersion::try_from,
            PacketVersion::get,
        ),
        (1, 0, 3),
    );
    assert_eq!(PacketVersion::SPACE_PACKET.get(), 0b000);
    assert_eq!(PacketVersion::ENCAPSULATION.get(), 0b111);
}

/// The Spacecraft Identifier widths the codecs use: 10 bits in TM (bits 2–11, 132.0
/// §4.1.2.2.3.1) and TC (bits 6–15, 232.0 §4.1.2.5.1), 8 bits in an AOS frame of Issue ≤ 4
/// (bits 2–9, the 8 least significant bits of 732.0 §4.1.2.2.3.1), and 16 bits in USLP
/// (bits 4–19, 732.1 §4.1.2.2.3).
#[test]
fn spacecraft_id_accepts_exactly_its_width() {
    // restricted: SpacecraftId
    // req: API-001, API-002, RS-010, VER-023
    check_id(
        field_max(10),
        SpacecraftId::<10>::MAX,
        field::SPACECRAFT_ID,
        (
            SpacecraftId::<10>::new,
            SpacecraftId::<10>::try_from,
            SpacecraftId::get,
        ),
        (6, 2, 10),
    );
    check_id(
        field_max(8),
        SpacecraftId::<8>::MAX,
        field::SPACECRAFT_ID,
        (
            SpacecraftId::<8>::new,
            SpacecraftId::<8>::try_from,
            SpacecraftId::get,
        ),
        (6, 2, 8),
    );
    check_id(
        field_max(16),
        SpacecraftId::<16>::MAX,
        field::SPACECRAFT_ID,
        (
            SpacecraftId::<16>::new,
            SpacecraftId::<16>::try_from,
            SpacecraftId::get,
        ),
        (7, 4, 16),
    );
}

/// The Virtual Channel Identifier widths: 3 bits in TM (bits 12–14, 132.0 §4.1.2.3), 6 bits in
/// TC (bits 16–21, 232.0 §4.1.2.6.1), AOS (bits 10–15, 732.0 §4.1.2.3.1), and USLP (bits 21–26,
/// 732.1 §4.1.2.4.1).
#[test]
fn vc_id_accepts_exactly_its_width() {
    // restricted: VcId
    // req: API-001, API-002, RS-010, VER-023
    check_id(
        field_max(3),
        VcId::<3>::MAX,
        field::VC_ID,
        (VcId::<3>::new, VcId::<3>::try_from, VcId::get),
        (6, 12, 3),
    );
    check_id(
        field_max(6),
        VcId::<6>::MAX,
        field::VC_ID,
        (VcId::<6>::new, VcId::<6>::try_from, VcId::get),
        (5, 16, 6),
    );
}

/// The MAP Identifier widths: 6 bits in the TC Segment Header (bits 2–7, 232.0 §4.1.3.2.2.3.1)
/// and 4 bits in USLP (bits 27–30, 732.1 §4.1.2.5.1).
#[test]
fn map_id_accepts_exactly_its_width() {
    // restricted: MapId
    // req: API-001, API-002, RS-010, VER-023
    check_id(
        field_max(6),
        MapId::<6>::MAX,
        field::MAP_ID,
        (MapId::<6>::new, MapId::<6>::try_from, MapId::get),
        (1, 2, 6),
    );
    check_id(
        field_max(4),
        MapId::<4>::MAX,
        field::MAP_ID,
        (MapId::<4>::new, MapId::<4>::try_from, MapId::get),
        (7, 27, 4),
    );
}

/// A modular counter has a width of 1–64 bits (CMN-030; the frame and packet counts of 132.0,
/// 732.0, 732.1, and 133.0 §4.1.3.4.3.4 all fall in it) and a value that fits the width. Every
/// width and the edges of every width's values, over the whole `u8` domain of the width.
#[test]
fn mod_counter_accepts_exactly_1_to_64_bits() {
    // restricted: ModCounter
    // req: CMN-030, GEN-024, RS-020, VER-023
    for bits in 0..=u8::MAX {
        if bits == 0 || bits > 64 {
            let limit = if bits == 0 { 1 } else { 64 };
            for value in [0, 1, u64::MAX] {
                assert_eq!(
                    ModCounter::new(bits, value),
                    Err(out_of_range(field::BITS, limit, u64::from(bits))),
                    "width {bits}"
                );
            }
            continue;
        }
        let max = u64::MAX >> (64 - u32::from(bits));
        for value in [0, max / 2, max] {
            let c = ModCounter::new(bits, value).unwrap();
            assert_eq!((c.bits(), c.value()), (bits, value));
        }
        if bits < 64 {
            for value in [max + 1, u64::MAX] {
                assert_eq!(
                    ModCounter::new(bits, value),
                    Err(out_of_range(field::VALUE, max, value)),
                    "width {bits}, value {value}"
                );
            }
        }
    }
}

/// The first two bits of the Operational Control Field classify its report: Type Flag `0` is a
/// Type-1 Report, `10` a project-specific Type-2 Report, `11` an SDLS Frame Security Report
/// (132.0 §4.1.5.4–5, 732.0 §4.1.5.4–5, 732.1 §4.1.5.2.1–2). Every first octet, each with the
/// other three octets all zeros and all ones.
#[test]
fn ocf_reports_accept_exactly_their_flag_bits() {
    // restricted: Type1Report, ProjectSpecificReport, FsrReport
    // req: CMN-010, RS-020, VER-023
    let wrong = |field: FieldId, expected: u64, found: u64| {
        Error::new(ErrorKind::InvalidField)
            .with_field(field)
            .with_detail(ErrorDetail::Expected { expected, found })
    };
    for first in 0..=u8::MAX {
        for rest in [0x00, 0xFF] {
            let octets = [first, rest, rest, rest];
            let (type_flag, report_use) = (first >> 7, (first >> 6) & 1);
            check_ocf(octets);
            let ocf = Ocf::decode(octets);
            assert_eq!(ocf.to_octets(), octets, "the OCF is its four octets");
            match (type_flag, report_use) {
                (0, _) => {
                    assert_eq!(ProjectSpecificReport::new(octets), None);
                    assert_eq!(
                        ProjectSpecificReport::try_from(octets),
                        Err(wrong(field::OCF_TYPE_FLAG, 1, 0))
                    );
                    assert_eq!(FsrReport::new(octets), None);
                    assert_eq!(
                        FsrReport::try_from(octets),
                        Err(wrong(field::OCF_TYPE_FLAG, 1, 0))
                    );
                }
                (_, 0) => {
                    assert_eq!(Type1Report::new(octets), None);
                    assert_eq!(
                        Type1Report::try_from(octets),
                        Err(wrong(field::OCF_TYPE_FLAG, 0, 1))
                    );
                    assert_eq!(FsrReport::new(octets), None);
                    assert_eq!(
                        FsrReport::try_from(octets),
                        Err(wrong(field::OCF_REPORT_USE, 1, 0))
                    );
                }
                _ => {
                    assert_eq!(Type1Report::new(octets), None);
                    assert_eq!(
                        Type1Report::try_from(octets),
                        Err(wrong(field::OCF_TYPE_FLAG, 0, 1))
                    );
                    assert_eq!(ProjectSpecificReport::new(octets), None);
                    assert_eq!(
                        ProjectSpecificReport::try_from(octets),
                        Err(wrong(field::OCF_REPORT_USE, 0, 1))
                    );
                }
            }
        }
    }
}

/// The report types of `octets` agree with `Ocf::decode`, and the accepted one gives the octets
/// back (CMN-010).
fn check_ocf(octets: [u8; 4]) {
    match Ocf::decode(octets) {
        Ocf::Clcw(r) => {
            assert_eq!(Type1Report::new(octets), Some(r));
            assert_eq!(Type1Report::try_from(octets), Ok(r));
            assert_eq!(r.to_octets(), octets);
            assert_eq!(<[u8; 4]>::from(r), octets);
        }
        Ocf::ProjectSpecific(r) => {
            assert_eq!(ProjectSpecificReport::new(octets), Some(r));
            assert_eq!(ProjectSpecificReport::try_from(octets), Ok(r));
            assert_eq!(r.to_octets(), octets);
            assert_eq!(<[u8; 4]>::from(r), octets);
        }
        Ocf::SdlsFsr(r) => {
            assert_eq!(FsrReport::new(octets), Some(r));
            assert_eq!(FsrReport::try_from(octets), Ok(r));
            assert_eq!(r.to_octets(), octets);
            assert_eq!(<[u8; 4]>::from(r), octets);
        }
    }
    let accepted = [
        Type1Report::new(octets).is_some(),
        ProjectSpecificReport::new(octets).is_some(),
        FsrReport::new(octets).is_some(),
    ];
    assert_eq!(
        accepted.iter().filter(|a| **a).count(),
        1,
        "exactly one report type"
    );
}

/// The `(code, name)` pairs of a vector file whose cases carry them: the error catalogue
/// (`core/errors.json`, copied from 03 §9) or the field catalogue (`core/fields.json`,
/// generated from 03 §9.1).
fn catalogue(file: &str) -> BTreeMap<u16, String> {
    let path = vectors_dir().join(file);
    let text = std::fs::read_to_string(&path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut out = BTreeMap::new();
    if let Some(fields) = json["fields"].as_array() {
        for f in fields {
            let code = u16::try_from(f["code"].as_u64().unwrap()).unwrap();
            out.insert(code, f["name"].as_str().unwrap().to_owned());
        }
    } else {
        for case in json["cases"].as_array().unwrap() {
            if case["operation"] == "error_code" && case["expect"]["ok"] == true {
                let code = u16::try_from(case["expect"]["code"].as_u64().unwrap()).unwrap();
                out.insert(code, case["params"]["kind"].as_str().unwrap().to_owned());
            }
        }
    }
    assert!(!out.is_empty(), "{file} has a catalogue");
    out
}

/// `ErrorKind::from_code` accepts exactly the codes of the error catalogue (03 §9; there is no
/// Blue Book clause, the catalogue is Aldis's), every `u16`, and gives the kind whose code and
/// name are the catalogue's: the code is the kind's wire form across the C ABI (CABI-020).
#[test]
fn error_kind_accepts_exactly_the_catalogue() {
    // restricted: ErrorKind
    // req: GEN-040, RS-020, VER-023
    let kinds = catalogue("core/errors.json");
    assert_eq!(kinds.len(), ErrorKind::ALL.len());
    for code in 0..=u16::MAX {
        match (ErrorKind::from_code(code), kinds.get(&code)) {
            (Some(kind), Some(name)) => {
                assert_eq!(kind.code(), code);
                assert_eq!(kind.name(), name);
                assert_eq!(ErrorKind::from_name(name), Some(kind));
            }
            (None, None) => {}
            (got, want) => panic!("code {code}: {got:?}, catalogue {want:?}"),
        }
    }
}

/// `FieldId::from_code` accepts exactly the codes of the field catalogue (03 §9.1, through its
/// generated copy `core/fields.json`), every `u16`, and gives the field with that code, the
/// field's wire form across the C ABI (CABI-021). Names are not compared (API-082).
#[test]
fn field_id_accepts_exactly_the_catalogue() {
    // restricted: FieldId
    // req: API-082, CABI-021, RS-020, VER-023
    let fields = catalogue("core/fields.json");
    assert_eq!(fields.len(), field::ALL.len());
    for code in 0..=u16::MAX {
        match FieldId::from_code(code) {
            Some(id) => {
                assert!(fields.contains_key(&code), "code {code} is catalogued");
                assert_eq!(id.code().get(), code);
            }
            None => assert!(!fields.contains_key(&code), "code {code} is found"),
        }
    }
}

/// A Security Header has at most 64 octets (355.0 §4.1.1.1.4). 355.0 §4.1.2.3 sets no bound on
/// the Security Trailer; its bound is the implementation limit `MAX_TRAILER_LEN`, 65 535 octets
/// (API-071). Every header length 0–300 and every trailer length 0–70 000, with the other at 0,
/// and `usize::MAX` for each.
#[test]
fn security_layout_accepts_exactly_its_lengths() {
    // restricted: SecurityLayout
    // req: API-070, API-071, RS-020, VER-023
    const SPEC_MAX_HEADER: usize = 64;
    assert_eq!(SecurityLayout::MAX_HEADER_LEN, SPEC_MAX_HEADER);
    assert_eq!(SecurityLayout::MAX_TRAILER_LEN, usize::from(u16::MAX));
    let header_err = Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_HEADER_LEN);
    let trailer_err = Error::new(ErrorKind::InvalidConfig).with_field(field::SECURITY_TRAILER_LEN);
    for h in (0..=300).chain([usize::MAX]) {
        match SecurityLayout::new(h, 0) {
            Ok(l) => {
                assert!(h <= SPEC_MAX_HEADER, "{h} accepted");
                assert_eq!((l.header_len(), l.trailer_len()), (h, 0));
            }
            Err(e) => {
                assert!(h > SPEC_MAX_HEADER, "{h} rejected");
                assert_eq!(e, header_err);
            }
        }
    }
    for t in (0..=70_000).chain([usize::MAX]) {
        match SecurityLayout::new(0, t) {
            Ok(l) => {
                assert!(t <= SecurityLayout::MAX_TRAILER_LEN, "{t} accepted");
                assert_eq!((l.header_len(), l.trailer_len()), (0, t));
            }
            Err(e) => {
                assert!(t > SecurityLayout::MAX_TRAILER_LEN, "{t} rejected");
                assert_eq!(e, trailer_err);
            }
        }
    }
    // A header beyond its bound is reported first.
    assert_eq!(SecurityLayout::new(65, 70_000), Err(header_err));
}

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    /// Valid identifiers drawn by the shared strategies round-trip through their fields at the
    /// clause's positions (133.0 §4.1.3; 132.0 §4.1.2.2.3, §4.1.2.3; 232.0 §4.1.3.2.2.3.1;
    /// 732.1 §4.1.2.2.3, §4.1.2.4.1, §4.1.2.5.1), and the strategies' out-of-range values are
    /// rejected with the documented error (API-001, VER-054).
    #[test]
    fn identifiers_from_the_strategies_round_trip(
        a in apid(),
        s in seq_count(),
        v in packet_version(),
        sc10 in spacecraft_id::<10>(),
        sc16 in spacecraft_id::<16>(),
        vc3 in vc_id::<3>(),
        vc6 in vc_id::<6>(),
        map6 in map_id::<6>(),
        map4 in map_id::<4>(),
        bad_apid in apid_out_of_range(),
        bad_count in seq_count_out_of_range(),
        bad_version in packet_version_out_of_range(),
    ) {
        // restricted: Apid, SeqCount, PacketVersion, SpacecraftId, VcId, MapId
        // req: API-001, API-002, RS-010, VER-023, VER-054
        prop_assert_eq!(through_field(6, 5, 11, a.get().into()), u64::from(a.get()));
        prop_assert_eq!(through_field(6, 18, 14, s.get().into()), u64::from(s.get()));
        prop_assert_eq!(through_field(1, 0, 3, v.get().into()), u64::from(v.get()));
        prop_assert_eq!(through_field(6, 2, 10, sc10.get().into()), u64::from(sc10.get()));
        prop_assert_eq!(through_field(7, 4, 16, sc16.get().into()), u64::from(sc16.get()));
        prop_assert_eq!(through_field(6, 12, 3, vc3.get().into()), u64::from(vc3.get()));
        prop_assert_eq!(through_field(5, 16, 6, vc6.get().into()), u64::from(vc6.get()));
        prop_assert_eq!(through_field(1, 2, 6, map6.get().into()), u64::from(map6.get()));
        prop_assert_eq!(through_field(7, 27, 4, map4.get().into()), u64::from(map4.get()));
        prop_assert_eq!(
            Apid::try_from(bad_apid),
            Err(out_of_range(field::APID, field_max(11), bad_apid.into()))
        );
        prop_assert_eq!(
            SeqCount::try_from(bad_count),
            Err(out_of_range(field::SEQUENCE_COUNT, field_max(14), bad_count.into()))
        );
        prop_assert_eq!(
            PacketVersion::try_from(bad_version),
            Err(out_of_range(field::PACKET_VERSION, field_max(3), bad_version.into()))
        );
    }

    /// A valid counter round-trips through a field of its width (the counts of 132.0 §4.1.2.5–6
    /// and 133.0 §4.1.3.4.3 are such fields), and an invalid width or value is rejected with
    /// the documented `OutOfRange` (CMN-030).
    #[test]
    fn mod_counters_round_trip_and_reject(
        c in mod_counter(),
        offset in 0_usize..8,
        (bad_bits, bad_value) in mod_counter_out_of_range(),
    ) {
        // restricted: ModCounter
        // req: CMN-030, GEN-024, VER-023, VER-054
        let back = through_field(9, offset, c.bits(), c.value());
        prop_assert_eq!(ModCounter::new(c.bits(), back), Ok(c));
        let expected = match bad_bits {
            0 => out_of_range(field::BITS, 1, 0),
            65.. => out_of_range(field::BITS, 64, bad_bits.into()),
            _ => out_of_range(field::VALUE, field_max(bad_bits.into()), bad_value),
        };
        prop_assert_eq!(ModCounter::new(bad_bits, bad_value), Err(expected));
    }

    /// Any four OCF octets are exactly one report type, whose octets are the OCF's
    /// (132.0 §4.1.5.4–5; CMN-010).
    #[test]
    fn ocf_reports_from_the_strategies(octets in ocf_octets()) {
        // restricted: Type1Report, ProjectSpecificReport, FsrReport
        // req: CMN-010, VER-023, VER-054
        check_ocf(octets);
    }

    /// `SecuredFields::new` accepts exactly three ranges that are each in order and adjacent,
    /// Security Header, data field, Security Trailer (355.0 §4.1), and rejects the others with
    /// `InvalidField` (`secured_fields`); the fields of every computed frame layout are
    /// accepted unchanged (CMN-040, API-070).
    #[test]
    fn secured_fields_accept_exactly_adjacent_ranges(
        [header, data, trailer] in secured_ranges(),
        spec in layout_spec(),
    ) {
        // restricted: SecuredFields
        // req: API-070, CMN-040, RS-020, VER-023, VER-054
        let valid = header.start <= header.end
            && data.start <= data.end
            && trailer.start <= trailer.end
            && header.end == data.start
            && data.end == trailer.start;
        match SecuredFields::new(header.clone(), data.clone(), trailer.clone()) {
            Ok(f) => {
                prop_assert!(valid);
                prop_assert_eq!((f.header(), f.data(), f.trailer()), (header, data, trailer));
            }
            Err(e) => {
                prop_assert!(!valid);
                prop_assert_eq!(
                    e,
                    Error::new(ErrorKind::InvalidField).with_field(field::SECURED_FIELDS)
                );
            }
        }
        if let Ok(layout) = FrameLayout::compute(&spec) {
            let from = SecuredFields::from_layout(&layout);
            let new = SecuredFields::new(
                layout.security_header(),
                layout.data(),
                layout.security_trailer(),
            );
            prop_assert_eq!(new, Ok(from));
        }
    }

    /// A Security Layout's lengths are the lengths of the Security Header and Trailer of every
    /// frame laid out with it (API-070, CMN-040): its wire form.
    #[test]
    fn security_layouts_size_the_frame_fields(spec in layout_spec()) {
        // restricted: SecurityLayout
        // req: API-070, API-071, CMN-040, VER-023, VER-054
        let s = spec.security;
        prop_assert_eq!(
            SecurityLayout::new(s.header_len(), s.trailer_len()),
            Ok(s)
        );
        if let Ok(layout) = FrameLayout::compute(&spec) {
            prop_assert_eq!(layout.security_header().len(), s.header_len());
            prop_assert_eq!(layout.security_trailer().len(), s.trailer_len());
        }
    }
}
