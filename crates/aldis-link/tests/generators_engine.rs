//! Configuration, refusals, reconfiguration, bounds, and round trips of the `AccessGenerator`
//! and the `BitstreamGenerator` on the shared link types (LNK-002, LNK-003, LNK-020, LNK-021,
//! LNK-074, LNK-075, LNK-078, API-052, API-056, API-058).
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    unused_must_use,
    reason = "tests fail loudly by design, and many steps are run for their effect on the state"
)]

use aldis_core::ErrorKind;
use aldis_core::field;
use aldis_core::ids::{MapId, VcId};
use aldis_link::access::{
    ACCESS_GENERATOR_MAX_ACTIONS, AccessGenerator, AccessGeneratorConfig, AccessInput, AccessOutput,
};
use aldis_link::bitstream::{
    BITSTREAM_GENERATOR_MAX_ACTIONS, BitstreamGenerator, BitstreamGeneratorConfig, BitstreamInput,
    BitstreamOutput,
};
use aldis_link::common::Qos;
use aldis_link::protocol::{Aos, Tm, Uslp, UslpParams};
use aldis_testkit::strategies;
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_uslp::Upid;

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/generators_engine.txt"
);

fn kind<T: std::fmt::Debug>(r: Result<T, aldis_core::Error>) -> ErrorKind {
    r.unwrap_err().kind()
}

fn named<T: std::fmt::Debug>(r: Result<T, aldis_core::Error>) -> (ErrorKind, Option<&'static str>) {
    let e = r.unwrap_err();
    (e.kind(), e.field().map(aldis_core::FieldId::name))
}

fn uslp_params() -> UslpParams {
    UslpParams::new(Upid::Packets, MapId::<4>::new(2).unwrap(), Qos::Expedited).unwrap()
}

fn uslp_config(field_len: usize) -> AccessGeneratorConfig<Uslp> {
    AccessGeneratorConfig::<Uslp>::new(VcId::<6>::new(2).unwrap(), uslp_params(), field_len)
        .unwrap()
}

#[test]
fn an_access_configuration_the_blue_books_forbid_is_refused() {
    // req: LNK-078, LNK-020, API-040
    let invalid = |f| (ErrorKind::InvalidConfig, Some(f));
    let tm = |len| AccessGeneratorConfig::<Tm>::new(VcId::<3>::new(1).unwrap(), (), len);
    assert_eq!(named(tm(0)), invalid(field::FIELD_LEN.name()));
    assert!(tm(1).is_ok());
    // A field above 65 535 octets would queue the length of its SDU modulo 65 536 (LNK-020).
    assert!(tm(65_535).is_ok());
    assert_eq!(named(tm(65_536)), invalid(field::FIELD_LEN.name()));
    let aos = |len| AccessGeneratorConfig::<Aos>::new(VcId::<6>::new(1).unwrap(), (), len);
    assert!(aos(65_535).is_ok());
    assert_eq!(named(aos(70_000)), invalid(field::FIELD_LEN.name()));
    // The header and at least one octet of zone; the zone's last octet needs a pointer value.
    let uslp =
        |len| AccessGeneratorConfig::<Uslp>::new(VcId::<6>::new(2).unwrap(), uslp_params(), len);
    assert_eq!(named(uslp(3)), invalid(field::FIELD_LEN.name()));
    assert!(uslp(4).is_ok());
    assert!(uslp(3 + 65_535).is_ok());
    assert_eq!(named(uslp(3 + 65_536)), invalid(field::FIELD_LEN.name()));
    // The longest SDU: 1 to 65 535 octets, and exactly the field where one SDU is one field.
    let config = uslp_config(11);
    assert_eq!(config.max_sdu_len(), 8);
    assert_eq!(
        named(config.with_max_sdu_len(0)),
        invalid(field::MAX_SDU_LEN.name())
    );
    assert_eq!(
        named(config.with_max_sdu_len(65_536)),
        invalid(field::MAX_SDU_LEN.name())
    );
    assert_eq!(config.with_max_sdu_len(40).unwrap().max_sdu_len(), 40);
    let config = tm(12).unwrap();
    assert_eq!(
        named(config.with_max_sdu_len(11)),
        invalid(field::MAX_SDU_LEN.name())
    );
    assert!(config.with_max_sdu_len(12).is_ok());
    assert_eq!(config.with_idle_pattern(0xA5).idle_pattern(), 0xA5);
    assert_eq!((config.vc().get(), config.field_len()), (1, 12));
    // The queue holds the longest SDU and its length.
    let long = uslp_config(11).with_max_sdu_len(40).unwrap();
    assert_eq!(
        named(AccessGenerator::<Uslp, 41>::new(long)),
        (ErrorKind::CapacityExceeded, Some(field::MAX_SDU_LEN.name()))
    );
    assert!(AccessGenerator::<Uslp, 42>::new(long).is_ok());
}

fn field_of(a: &aldis_link::access::AccessActions, out: &[u8]) -> Option<Vec<u8>> {
    match a.get(0) {
        Some(AccessOutput::DataField(f)) => Some(f.field.slice(out).unwrap().to_vec()),
        _ => None,
    }
}

#[test]
fn an_sdu_the_generator_cannot_take_is_refused_and_changes_nothing() {
    // req: LNK-020, LNK-002, API-052, API-058, API-056
    let config = uslp_config(11).with_max_sdu_len(20).unwrap();
    let mut g = AccessGenerator::<Uslp, 40>::new(config).unwrap();
    let mut out = [0; 11];
    let mut submit = |g: &mut AccessGenerator<Uslp, 40>, n: usize| {
        g.step(AccessInput::SubmitSdu(&vec![7; n]), &mut out)
    };
    assert_eq!(
        named(submit(&mut g, 0)),
        (ErrorKind::LengthMismatch, Some("sdu_len"))
    );
    assert_eq!(
        named(submit(&mut g, 21)),
        (ErrorKind::OutOfRange, Some("max_sdu_len"))
    );
    submit(&mut g, 20).unwrap();
    // 22 octets are queued; the 18 left take an SDU of 16 and its length, not one of 17.
    assert_eq!(kind(submit(&mut g, 17)), ErrorKind::WouldBlock);
    submit(&mut g, 16).unwrap();
    let s = g.status();
    assert_eq!(
        (s.sdus_in.get(), s.refused.get(), s.would_block.get()),
        (2, 2, 1)
    );
    assert_eq!((s.queue.current(), s.queue.high_water()), (36, 36));
    // A field needs the whole output area.
    let mut short = [0; 10];
    assert_eq!(
        kind(g.step(AccessInput::Release, &mut short)),
        ErrorKind::BufferTooSmall
    );
    assert_eq!(g.status().fields_out.get(), 0);
    // The refusals changed nothing: the first field starts the first SDU.
    let a = g.step(AccessInput::Release, &mut out).unwrap();
    assert_eq!(
        field_of(&a, &out).unwrap(),
        [0x20, 0xFF, 0xFF, 7, 7, 7, 7, 7, 7, 7, 7]
    );
    // Reconfigure discards what is left of the first SDU and the second.
    let a = g.step(AccessInput::Reconfigure(config), &mut out).unwrap();
    assert_eq!(a.get(0), Some(&AccessOutput::Discarded { octets: 12 + 16 }));
    assert_eq!(g.status().discarded_octets.get(), 28);
    assert!(g.step(AccessInput::Release, &mut out).unwrap().is_empty());
    assert!(
        g.step(AccessInput::Reconfigure(config), &mut out)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        named(g.step(
            AccessInput::Reconfigure(config.with_max_sdu_len(39).unwrap()),
            &mut out
        )),
        (ErrorKind::CapacityExceeded, Some("max_sdu_len"))
    );
    assert_eq!(ACCESS_GENERATOR_MAX_ACTIONS, 1);
    assert_eq!(g.config().field_len(), 11);
}

#[test]
fn the_queue_frees_what_a_release_takes() {
    // req: LNK-020, API-052
    let config = AccessGeneratorConfig::<Tm>::new(VcId::<3>::new(1).unwrap(), (), 4).unwrap();
    let mut g = AccessGenerator::<Tm, 12>::new(config).unwrap();
    let mut out = [0; 4];
    let sdu = |n: u8| [n; 4];
    g.step(AccessInput::SubmitSdu(&sdu(1)), &mut out).unwrap();
    g.step(AccessInput::SubmitSdu(&sdu(2)), &mut out).unwrap();
    assert_eq!(
        kind(g.step(AccessInput::SubmitSdu(&sdu(3)), &mut out)),
        ErrorKind::WouldBlock
    );
    let a = g.step(AccessInput::Release, &mut out).unwrap();
    assert_eq!(field_of(&a, &out).unwrap(), sdu(1));
    g.step(AccessInput::SubmitSdu(&sdu(3)), &mut out).unwrap();
    for n in [2, 3] {
        let a = g.step(AccessInput::Release, &mut out).unwrap();
        assert_eq!(field_of(&a, &out).unwrap(), sdu(n));
    }
    assert!(g.step(AccessInput::Release, &mut out).unwrap().is_empty());
}

/// Reassembles SDUs from the fields of rules `001` and `010` with the Last Valid Octet Pointer.
fn reassemble(fields: &[Vec<u8>], sdus: &mut Vec<Vec<u8>>, current: &mut Vec<u8>) {
    for f in fields {
        let rule = f[0] >> 5;
        let lvo = u16::from_be_bytes([f[1], f[2]]);
        assert_eq!(rule, if current.is_empty() { 0b001 } else { 0b010 });
        if lvo == 0xFFFF {
            current.extend_from_slice(&f[3..]);
        } else {
            current.extend_from_slice(&f[3..=3 + usize::from(lvo)]);
            sdus.push(std::mem::take(current));
        }
    }
}

proptest! {
    #![proptest_config(strategies::config(128, REGRESSIONS))]

    #[test]
    fn sdus_come_out_of_the_fields_as_they_went_in(
        sdus in prop::collection::vec(prop::collection::vec(any::<u8>(), 1..40), 1..6),
        field_len in 4_usize..20,
    ) {
        // req: LNK-020, LNK-074, LNK-075, VER-023
        let config = uslp_config(field_len).with_max_sdu_len(40).unwrap();
        let mut g = AccessGenerator::<Uslp, 256>::new(config).unwrap();
        let mut out = vec![0_u8; field_len];
        for sdu in &sdus {
            g.step(AccessInput::SubmitSdu(sdu), &mut out).unwrap();
        }
        let mut fields = Vec::new();
        loop {
            let a = g.step(AccessInput::Release, &mut out).unwrap();
            prop_assert!(a.len() <= ACCESS_GENERATOR_MAX_ACTIONS);
            match field_of(&a, &out) {
                Some(f) => fields.push(f),
                None => break,
            }
        }
        let (mut got, mut current) = (Vec::new(), Vec::new());
        reassemble(&fields, &mut got, &mut current);
        prop_assert!(current.is_empty());
        prop_assert_eq!(got, sdus);
    }
}

fn bit_config(field_len: usize, idle: u8) -> BitstreamGeneratorConfig<Aos> {
    BitstreamGeneratorConfig::<Aos>::new(VcId::<6>::new(3).unwrap(), field_len, idle).unwrap()
}

#[test]
fn a_bitstream_configuration_the_blue_books_forbid_is_refused() {
    // req: LNK-078, LNK-021, AOS-018, API-040
    let make = |len| BitstreamGeneratorConfig::<Aos>::new(VcId::<6>::new(3).unwrap(), len, 0);
    for len in [0, 2] {
        assert_eq!(
            named(make(len)),
            (ErrorKind::InvalidConfig, Some("field_len"))
        );
    }
    assert!(make(3).is_ok());
    assert!(make(2 + 2047).is_ok());
    assert_eq!(
        named(make(2 + 2048)),
        (ErrorKind::InvalidConfig, Some("field_len"))
    );
    let c = bit_config(6, 0xFF);
    assert_eq!(
        (c.vc().get(), c.field_len(), c.idle_pattern()),
        (3, 6, 0xFF)
    );
}

#[test]
fn bits_the_generator_cannot_take_are_refused_and_change_nothing() {
    // req: LNK-021, LNK-002, API-052, API-058, API-056
    let mut g = BitstreamGenerator::<Aos, 2>::new(bit_config(6, 0));
    let mut out = [0; 6];
    let submit = |g: &mut BitstreamGenerator<Aos, 2>, bits: &[u8], n| {
        g.step(BitstreamInput::SubmitBits { bits, bit_len: n }, &mut [0; 6])
    };
    assert_eq!(
        named(submit(&mut g, &[0xFF], 0)),
        (ErrorKind::LengthMismatch, Some("sdu_len"))
    );
    assert_eq!(
        named(submit(&mut g, &[0xFF], 9)),
        (ErrorKind::LengthMismatch, Some("sdu_len"))
    );
    submit(&mut g, &[0xFF, 0xFF], 11).unwrap();
    // 11 of 16 bits are queued.
    assert_eq!(kind(submit(&mut g, &[0xFF], 6)), ErrorKind::WouldBlock);
    submit(&mut g, &[0xFF], 5).unwrap();
    let s = g.status();
    assert_eq!(
        (s.submissions.get(), s.refused.get(), s.would_block.get()),
        (2, 2, 1)
    );
    assert_eq!((s.queue.current(), s.queue.high_water()), (2, 2));
    let mut short = [0; 5];
    assert_eq!(
        kind(g.step(BitstreamInput::Release, &mut short)),
        ErrorKind::BufferTooSmall
    );
    let a = g.step(BitstreamInput::Release, &mut out).unwrap();
    let Some(BitstreamOutput::DataField(f)) = a.get(0) else {
        panic!("no field")
    };
    // 16 bits of a 32-bit zone: the pointer is the last valid bit.
    assert_eq!((f.vc, f.pointer, f.field.len()), (3, 15, 6));
    assert_eq!(out, [0x00, 0x0F, 0xFF, 0xFF, 0x00, 0x00]);
    // An idle B_PDU, then a Reconfigure of nothing queued.
    let a = g.step(BitstreamInput::Release, &mut out).unwrap();
    assert!(matches!(a.get(0), Some(BitstreamOutput::DataField(f)) if f.pointer == 0x3FFE));
    assert_eq!(g.status().idle_fields.get(), 1);
    assert!(
        g.step(BitstreamInput::Reconfigure(bit_config(6, 0)), &mut out)
            .unwrap()
            .is_empty()
    );
    // Bits discarded are counted in whole octets.
    submit(&mut g, &[0xFF, 0xFF], 9).unwrap();
    let a = g
        .step(BitstreamInput::Reconfigure(bit_config(8, 0xAA)), &mut out)
        .unwrap();
    assert_eq!(a.get(0), Some(&BitstreamOutput::Discarded { octets: 2 }));
    assert_eq!(g.status().discarded_octets.get(), 2);
    assert_eq!(g.config().field_len(), 8);
    assert_eq!(BITSTREAM_GENERATOR_MAX_ACTIONS, 1);
}

/// The valid bits of a released `B_PDU`.
fn valid_bits(field: &[u8], pointer: u16) -> Vec<bool> {
    let zone = &field[2..];
    let n = match pointer {
        0x3FFF => zone.len() * 8,
        0x3FFE => 0,
        p => usize::from(p) + 1,
    };
    (0..n)
        .map(|i| zone[i / 8] & (0x80 >> (i % 8)) != 0)
        .collect()
}

proptest! {
    #![proptest_config(strategies::config(128, REGRESSIONS))]

    #[test]
    fn bits_come_out_of_the_zones_as_they_went_in(
        chunks in prop::collection::vec((prop::collection::vec(any::<u8>(), 1..4), 1_usize..25), 1..6),
        field_len in 3_usize..8,
    ) {
        // req: LNK-021, LNK-074, LNK-075, VER-023
        let mut g = BitstreamGenerator::<Aos, 64>::new(bit_config(field_len, 0));
        let mut out = vec![0_u8; field_len];
        let mut sent = Vec::new();
        for (bytes, n) in &chunks {
            let n = (*n).min(bytes.len() * 8);
            g.step(BitstreamInput::SubmitBits { bits: bytes, bit_len: n }, &mut out).unwrap();
            sent.extend((0..n).map(|i| bytes[i / 8] & (0x80 >> (i % 8)) != 0));
        }
        let mut got = Vec::new();
        while got.len() < sent.len() {
            let a = g.step(BitstreamInput::Release, &mut out).unwrap();
            prop_assert!(a.len() <= BITSTREAM_GENERATOR_MAX_ACTIONS);
            let Some(BitstreamOutput::DataField(f)) = a.get(0) else {
                panic!("no field")
            };
            got.extend(valid_bits(&out, f.pointer));
        }
        prop_assert_eq!(got, sent);
    }
}
