//! Housekeeping, reconfiguration, bounds, and refusals of the `PacketGenerator` and the
//! `PacketExtractor` on the shared link types (LNK-002, LNK-003, LNK-010, LNK-015, LNK-074,
//! LNK-075, LNK-078, API-052, API-053, API-056, API-058).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    unused_must_use,
    reason = "tests fail loudly by design, and many steps are run for their effect on the state"
)]

use aldis_aos::AosEdition;
use aldis_core::ErrorKind;
use aldis_core::field;
use aldis_core::ids::{MapId, VcId};
use aldis_core::time::{Duration, Instant};
use aldis_link::common::{IdlePacketKind, IdlePolicy, Qos, ValidPvns};
use aldis_link::extractor::{
    ExtractionError, ExtractorInput, ExtractorOutput, FieldIn, PacketExtractor,
    PacketExtractorConfig, extractor_max_actions,
};
use aldis_link::generator::{
    GeneratorInput, GeneratorOutput, PACKET_GENERATOR_MAX_ACTIONS, PacketGenerator,
    PacketGeneratorConfig,
};
use aldis_link::protocol::Pointer::{self, NoStart, OnlyIdle, StartsAt};
use aldis_link::protocol::{
    Aos, AosParams, LinkProtocol, PacketProtocol, Protocol, Tc, Tm, TmParams, Uslp, UslpParams,
};
use aldis_uslp::Upid;

const T0: Instant = Instant::from_micros(0);

type TmGen = PacketGenerator<Tm, 24, 64>;
type TmExt = PacketExtractor<Tm, 100, 24, 40>;

/// A Space Packet of `total` octets (at least 7), APID 1, count 0, data `fill`.
fn spp(total: usize, fill: u8) -> Vec<u8> {
    let mut p = vec![0x00, 0x01, 0xC0, 0x00];
    p.extend_from_slice(&u16::try_from(total - 7).unwrap().to_be_bytes());
    p.resize(total, fill);
    p
}

fn tm_config(field_len: usize) -> PacketGeneratorConfig<Tm> {
    PacketGeneratorConfig::<Tm>::new(
        VcId::<3>::new(1).unwrap(),
        TmParams,
        field_len,
        ValidPvns::BOTH,
        64,
    )
    .unwrap()
}

fn tm_gen() -> TmGen {
    TmGen::new(tm_config(24)).unwrap()
}

fn release(g: &mut TmGen, now: Instant) -> Vec<u8> {
    let mut out = [0u8; 24];
    let a = g.step(GeneratorInput::Release, now, &mut out).unwrap();
    match a.get(0) {
        Some(GeneratorOutput::DataField(f)) => f.field.slice(&out).unwrap().to_vec(),
        other => panic!("no field: {other:?}"),
    }
}

fn ext_config() -> PacketExtractorConfig<Tm> {
    PacketExtractorConfig::<Tm>::new(TmParams, ValidPvns::BOTH, 40).unwrap()
}

const fn field_in(field: &[u8], pointer: u16) -> ExtractorInput<'_, Tm> {
    ExtractorInput::DataField(FieldIn {
        field,
        header_pointer: Some(pointer),
        gap: false,
    })
}

#[test]
fn the_generator_counts_what_it_does() {
    // req: LNK-002, API-056
    let mut g = tm_gen();
    let mut out = [0u8; 24];
    assert_eq!(g.status().queue.current(), 0);
    // 20 octets: 20 in the field, then 40 more: 4 in the field, 36 queued (a packet of 40 split).
    g.step(GeneratorInput::SubmitPacket(&spp(20, 1)), T0, &mut out)
        .unwrap();
    g.step(GeneratorInput::SubmitPacket(&spp(40, 2)), T0, &mut out)
        .unwrap();
    let s = *g.status();
    assert_eq!(s.packets_in.get(), 2);
    assert_eq!((s.field.current(), s.queue.current()), (24, 36));
    assert_eq!((s.field.high_water(), s.queue.high_water()), (24, 36));
    // A refused packet and a WouldBlock count, and change nothing else.
    let before = g.clone();
    let bad = g
        .step(
            GeneratorInput::SubmitPacket(&[0x20, 0, 0, 0, 0, 0, 0]),
            T0,
            &mut out,
        )
        .unwrap_err();
    assert_eq!(bad.kind(), ErrorKind::InvalidField);
    let blocked = g
        .step(GeneratorInput::SubmitPacket(&spp(40, 3)), T0, &mut out)
        .unwrap_err();
    assert_eq!(blocked.kind(), ErrorKind::WouldBlock);
    let s = *g.status();
    assert_eq!((s.refused.get(), s.would_block.get()), (1, 1));
    assert_eq!(g.config(), before.config());
    assert_eq!(
        (s.field.current(), s.queue.current(), s.packets_in.get()),
        (24, 36, 2)
    );
    // Two releases: the full field, then the rest completed by idle packets at the deadline.
    release(&mut g, T0);
    assert_eq!(g.status().queue.current(), 12);
    assert_eq!(g.status().fields_out.get(), 1);
    assert_eq!(g.status().queue.high_water(), 36);
}

#[test]
fn reconfigure_discards_and_keeps_the_counters() {
    // req: LNK-003, LNK-002
    let mut g = tm_gen();
    let mut out = [0u8; 24];
    g.step(GeneratorInput::SubmitPacket(&spp(30, 1)), T0, &mut out)
        .unwrap();
    let a = g
        .step(GeneratorInput::Reconfigure(tm_config(20)), T0, &mut out)
        .unwrap();
    assert_eq!(a.get(0), Some(&GeneratorOutput::Discarded { octets: 30 }));
    assert_eq!(a.len(), PACKET_GENERATOR_MAX_ACTIONS);
    let s = g.status();
    assert_eq!((s.packets_in.get(), s.discarded_octets.get()), (1, 30));
    assert_eq!((s.field.current(), s.queue.current()), (0, 0));
    assert_eq!(g.config().field_len(), 20);
    assert!(g.next_deadline().is_none());
    // Nothing held: no notification.
    let a = g
        .step(GeneratorInput::Reconfigure(tm_config(20)), T0, &mut out)
        .unwrap();
    assert!(a.is_empty());
    // A configuration that does not fit the capacities is refused and changes nothing.
    g.step(GeneratorInput::SubmitPacket(&spp(8, 1)), T0, &mut out)
        .unwrap();
    let e = g
        .step(GeneratorInput::Reconfigure(tm_config(25)), T0, &mut out)
        .unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::CapacityExceeded, Some(field::FIELD_LEN))
    );
    assert_eq!(g.status().field.current(), 8);
    assert_eq!(g.config().field_len(), 20);
}

#[test]
fn the_generator_refuses_what_the_blue_book_forbids() {
    // req: LNK-078
    let vc = VcId::<3>::new(1).unwrap();
    let bad = |field_len, pvns, max| {
        PacketGeneratorConfig::<Tm>::new(vc, TmParams, field_len, pvns, max).unwrap_err()
    };
    assert_eq!(
        (
            bad(0, ValidPvns::BOTH, 64).kind(),
            bad(0, ValidPvns::BOTH, 64).field()
        ),
        (ErrorKind::InvalidConfig, Some(field::FIELD_LEN))
    );
    // The pointer of TM holds offsets up to 0x7FD: a zone of 0x7FF octets does not fit.
    assert_eq!(
        bad(0x7FF, ValidPvns::BOTH, 64).field(),
        Some(field::FIELD_LEN)
    );
    assert_eq!(
        bad(24, ValidPvns::BOTH, 0).field(),
        Some(field::MAX_PACKET_LEN)
    );
    // A Space Packet idle packet needs 7 octets of zone.
    assert_eq!(
        bad(6, ValidPvns::SPACE, 64).field(),
        Some(field::IDLE_PACKET_KIND)
    );
    // The default idle packet is of a kind the set holds.
    let encap = PacketGeneratorConfig::<Tm>::new(vc, TmParams, 6, ValidPvns::ENCAPSULATION, 64);
    assert_eq!(
        encap.unwrap().idle_packet_kind(),
        IdlePacketKind::Encapsulation
    );
    // An idle packet of a PVN that is not valid.
    let e = tm_config(24)
        .with_idle(IdlePolicy::Oid, IdlePacketKind::Encapsulation, 0)
        .unwrap();
    assert!(e.valid_pvns().space());
    let only_space = PacketGeneratorConfig::<Tm>::new(vc, TmParams, 24, ValidPvns::SPACE, 64)
        .unwrap()
        .with_idle(IdlePolicy::Oid, IdlePacketKind::Encapsulation, 0)
        .unwrap_err();
    assert_eq!(only_space.field(), Some(field::IDLE_PACKET_KIND));
    // The capacities.
    let huge = PacketGeneratorConfig::<Tm>::new(vc, TmParams, 24, ValidPvns::BOTH, 100).unwrap();
    let e = TmGen::new(huge).unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::CapacityExceeded, Some(field::MAX_PACKET_LEN))
    );
    let e = PacketGenerator::<Tm, 16, 64>::new(tm_config(24)).unwrap_err();
    assert_eq!(e.field(), Some(field::FIELD_LEN));
}

#[test]
fn an_area_too_short_fails_with_no_change() {
    // req: LNK-075, API-058
    let mut g = tm_gen();
    let mut out = [0u8; 24];
    g.step(GeneratorInput::SubmitPacket(&spp(24, 1)), T0, &mut out)
        .unwrap();
    let before = g.clone();
    let mut short = [0u8; 23];
    let e = g.step(GeneratorInput::Release, T0, &mut short).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
    assert_eq!(e.detail(), aldis_core::ErrorDetail::Needed(24));
    assert_eq!(g, before);
    // The IdleField policy needs the area too.
    let idle = tm_config(24)
        .with_idle(IdlePolicy::IdleField, IdlePacketKind::SpacePacket, 0x11)
        .unwrap();
    let mut g = TmGen::new(idle).unwrap();
    let e = g.step(GeneratorInput::Release, T0, &mut short).unwrap_err();
    assert_eq!(e.detail(), aldis_core::ErrorDetail::Needed(24));
    assert_eq!(g.status().fields_out.get(), 0);
    g.step(GeneratorInput::Release, T0, &mut out).unwrap();
    assert_eq!(
        (g.status().fields_out.get(), g.status().idle_fields.get()),
        (1, 1)
    );
}

#[test]
fn timeouts_are_harmless_until_the_deadline() {
    // req: API-053, LNK-012
    let cfg = tm_config(24).with_release_deadline(Duration::from_micros(100));
    let mut g = TmGen::new(cfg).unwrap();
    let mut out = [0u8; 24];
    assert!(g.next_deadline().is_none());
    g.step(GeneratorInput::Timeout, T0, &mut out).unwrap();
    g.step(
        GeneratorInput::SubmitPacket(&spp(8, 1)),
        Instant::from_micros(10),
        &mut out,
    )
    .unwrap();
    assert_eq!(g.next_deadline(), Some(Instant::from_micros(110)));
    for at in [10, 50, 109] {
        g.step(GeneratorInput::Timeout, Instant::from_micros(at), &mut out)
            .unwrap();
        assert_eq!(g.next_deadline(), Some(Instant::from_micros(110)));
        assert!(
            g.step(GeneratorInput::Release, Instant::from_micros(at), &mut out)
                .unwrap()
                .is_empty()
        );
    }
    g.step(GeneratorInput::Timeout, Instant::from_micros(110), &mut out)
        .unwrap();
    assert!(g.next_deadline().is_none());
    // Repeated Timeouts change nothing; the field is released once.
    g.step(GeneratorInput::Timeout, Instant::from_micros(500), &mut out)
        .unwrap();
    let field = release(&mut g, Instant::from_micros(500));
    assert_eq!(field.len(), 24);
}

#[test]
fn a_packet_waits_in_the_queue_behind_a_marked_field() {
    // req: LNK-012, LNK-015, LNK-011
    let cfg = tm_config(24).with_release_deadline(Duration::from_micros(10));
    let mut g = TmGen::new(cfg).unwrap();
    let mut out = [0u8; 24];
    g.step(GeneratorInput::SubmitPacket(&spp(8, 1)), T0, &mut out)
        .unwrap();
    g.step(GeneratorInput::Timeout, Instant::from_micros(10), &mut out)
        .unwrap();
    // The field is releasable and takes no more octets: the packet goes to the queue.
    g.step(
        GeneratorInput::SubmitPacket(&spp(8, 2)),
        Instant::from_micros(11),
        &mut out,
    )
    .unwrap();
    assert_eq!(
        (g.status().field.current(), g.status().queue.current()),
        (8, 8)
    );
    let first = release(&mut g, Instant::from_micros(12));
    assert_eq!(&first[..8], &spp(8, 1)[..]);
    // The next field starts with the queued packet at the Release.
    assert_eq!(g.next_deadline(), Some(Instant::from_micros(22)));
    assert_eq!(g.status().queue.current(), 0);
}

#[test]
fn a_continuation_field_has_no_start() {
    // req: LNK-011
    let mut g = tm_gen();
    let mut out = [0u8; 24];
    // 60 octets: three fields, 24 + 24 + 12 (the last completed by idle at the deadline).
    g.step(GeneratorInput::SubmitPacket(&spp(60, 7)), T0, &mut out)
        .unwrap();
    let a = g.step(GeneratorInput::Release, T0, &mut out).unwrap();
    let Some(GeneratorOutput::DataField(f)) = a.get(0) else {
        panic!()
    };
    assert_eq!((f.pointer, f.vc, f.route), (0, 1, None));
    let a = g.step(GeneratorInput::Release, T0, &mut out).unwrap();
    let Some(GeneratorOutput::DataField(f)) = a.get(0) else {
        panic!()
    };
    assert_eq!(f.pointer, 0x7FF);
}

#[test]
fn the_extractor_refuses_what_it_cannot_take() {
    // req: LNK-075, API-058, LNK-060
    let mut e = TmExt::new(ext_config()).unwrap();
    let mut out = [0u8; 100];
    let field = [0u8; 24];
    // The area must hold max_packet_len - 1 plus the zone.
    let before = e.clone();
    let mut short = [0u8; 62];
    let err = e.step(field_in(&field, 0), &mut short).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::BufferTooSmall);
    assert_eq!(err.detail(), aldis_core::ErrorDetail::Needed(63));
    assert_eq!(e, before);
    // A TM field needs its pointer from the frame header; a pointer outside the zone is
    // malformed; a zone longer than D does not fit.
    let no_ptr = ExtractorInput::DataField(FieldIn {
        field: &field,
        header_pointer: None,
        gap: false,
    });
    let err = e.step(no_ptr, &mut out).unwrap_err();
    assert_eq!(
        (err.kind(), err.field()),
        (ErrorKind::InvalidField, Some(field::FIRST_HEADER_POINTER))
    );
    let err = e.step(field_in(&field, 24), &mut out).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Malformed);
    let long = [0u8; 25];
    let err = e.step(field_in(&long, 0), &mut out).unwrap_err();
    assert_eq!(
        (err.kind(), err.field()),
        (ErrorKind::CapacityExceeded, Some(field::FIELD_LEN))
    );
    assert_eq!(e, before);
}

#[test]
fn the_extractor_capacities_are_checked() {
    // req: LNK-074, GEN-011
    let c = ext_config();
    assert!(PacketExtractor::<Tm, 7, 24, 40>::new(c).is_err());
    assert!(PacketExtractor::<Tm, 39, 24, 40>::new(c).is_err());
    assert!(PacketExtractor::<Tm, 100, 24, 3>::new(c).is_err());
    assert_eq!(extractor_max_actions(24, 3), 1 + 23 / 3 + 3);
    assert_eq!(extractor_max_actions(24, 7), 1 + 23 / 7 + 3);
    assert_eq!(extractor_max_actions(24, 0), 4);
    assert_eq!(extractor_max_actions(0, 7), 4);
    assert_eq!(c.min_packet_len(), 3);
    assert_eq!(
        PacketExtractorConfig::<Tm>::new(TmParams, ValidPvns::SPACE, 40)
            .unwrap()
            .min_packet_len(),
        7
    );
    assert_eq!(c.with_delivery(false, true).min_packet_len(), 1);
    assert_eq!(
        PacketExtractorConfig::<Tm>::new(TmParams, ValidPvns::BOTH, 0)
            .unwrap_err()
            .field(),
        Some(field::MAX_PACKET_LEN)
    );
    // The bound of the engine type holds for the configuration of a reconfiguration too.
    let mut e = TmExt::new(c).unwrap();
    let mut out = [0u8; 100];
    let too_long = PacketExtractorConfig::<Tm>::new(TmParams, ValidPvns::BOTH, 101).unwrap();
    assert!(
        e.step(ExtractorInput::Reconfigure(too_long), &mut out)
            .is_err()
    );
}

#[test]
fn the_extractor_counts_and_reconfigures() {
    // req: LNK-002, LNK-003, LNK-061, LNK-062, LNK-063
    let mut e = TmExt::new(ext_config()).unwrap();
    let mut out = [0u8; 100];
    // Nothing held: no notification.
    let a = e
        .step(ExtractorInput::Reconfigure(ext_config()), &mut out)
        .unwrap();
    assert!(a.is_empty());
    let mut field = spp(24, 1);
    let a = e.step(field_in(&field, 0), &mut out).unwrap();
    assert_eq!(a.len(), 1);
    // A packet of 20 octets and the first 4 octets of another: held.
    field = spp(20, 2);
    field.extend_from_slice(&spp(30, 3)[..4]);
    e.step(field_in(&field, 0), &mut out).unwrap();
    assert_eq!(e.status().assembly.current(), 4);
    assert_eq!(e.status().packets_out.get(), 2);
    let a = e
        .step(ExtractorInput::Reconfigure(ext_config()), &mut out)
        .unwrap();
    assert_eq!(a.get(0), Some(&ExtractorOutput::Discarded { octets: 4 }));
    assert_eq!(e.status().discarded_octets.get(), 4);
    assert_eq!(e.status().assembly.high_water(), 24);
    assert_eq!(e.status().assembly.current(), 0);
    // An invalid PVN, a packet that is too long, an invalid length: counted.
    let mut synced = |field: &[u8]| {
        let mut e2 = TmExt::new(ext_config()).unwrap();
        let a = e2.step(field_in(field, 0), &mut out).unwrap();
        (a.iter().copied().collect::<Vec<_>>(), *e2.status())
    };
    let (a, s) = synced(&[0x20; 24]);
    assert_eq!(a, [ExtractorOutput::Error(ExtractionError::InvalidPvn)]);
    assert_eq!(s.invalid_pvn.get(), 1);
    let mut long = spp(24, 1);
    long[5] = 0x7F;
    let (a, s) = synced(&long);
    assert_eq!(a, [ExtractorOutput::Error(ExtractionError::PacketTooLong)]);
    assert_eq!(s.packet_too_long.get(), 1);
    // An Encapsulation Packet whose length is shorter than its header.
    let mut bad = vec![0xE1, 0x01];
    bad.resize(24, 0);
    let (a, s) = synced(&bad);
    assert_eq!(
        a,
        [ExtractorOutput::Error(ExtractionError::LengthInconsistent)]
    );
    assert_eq!(s.length_inconsistent.get(), 1);
}

#[test]
fn judgement_calls_of_oi_78_for_the_extractor() {
    // req: LNK-060, LNK-061, LNK-064
    let mut out = [0u8; 100];
    let first = {
        let mut f = spp(20, 1);
        f.extend_from_slice(&spp(30, 2)[..4]);
        f
    };
    // A gap on a field of only idle data discards the partial packet and loses synchronisation.
    let mut e = TmExt::new(ext_config()).unwrap();
    e.step(field_in(&first, 0), &mut out).unwrap();
    let idle = ExtractorInput::DataField(FieldIn {
        field: &[0; 24],
        header_pointer: Some(0x7FE),
        gap: true,
    });
    let a = e.step(idle, &mut out).unwrap();
    assert_eq!(a.get(0), Some(&ExtractorOutput::Discarded { octets: 4 }));
    assert_eq!(e.status().gaps.get(), 1);
    let a = e.step(field_in(&spp(24, 3), 0), &mut out).unwrap();
    assert_eq!(a.len(), 1, "a field with a pointer resynchronises");
    // A packet that ends inside a field in which none starts: the pointer is correct, the
    // packet is not delivered, and the extractor resynchronises at the next pointer.
    let mut e = TmExt::new(ext_config()).unwrap();
    let mut head = spp(30, 4);
    let tail = head.split_off(24);
    e.step(field_in(&head, 0), &mut out).unwrap();
    let mut field = tail;
    field.resize(24, 0xAA);
    let a = e.step(field_in(&field, 0x7FF), &mut out).unwrap();
    assert_eq!(
        a.iter().copied().collect::<Vec<_>>(),
        [ExtractorOutput::Error(ExtractionError::LengthInconsistent)]
    );
    let a = e.step(field_in(&field, 0x7FF), &mut out).unwrap();
    assert!(a.is_empty());
    // A partial packet whose header is incomplete is discarded, not delivered incomplete, and so
    // is a partial idle packet.
    let cfg = ext_config().with_delivery(true, false);
    let mut e = TmExt::new(cfg).unwrap();
    let mut f = spp(20, 1);
    f.extend_from_slice(&[0x00, 0x01, 0xC0]);
    e.step(field_in(&f, 0), &mut out).unwrap();
    let gap = ExtractorInput::DataField(FieldIn {
        field: &[0; 24],
        header_pointer: Some(0x7FF),
        gap: true,
    });
    let a = e.step(gap, &mut out).unwrap();
    assert_eq!(a.get(0), Some(&ExtractorOutput::Discarded { octets: 3 }));
    let mut idle_head = spp(17, 1);
    idle_head.extend_from_slice(&[0x07, 0xFF, 0xC0, 0x00, 0x00, 0x10, 0x00]);
    let _ = e.step(field_in(&idle_head, 0), &mut out).unwrap();
    let a = e
        .step(
            ExtractorInput::DataField(FieldIn {
                field: &[0; 24],
                header_pointer: Some(0x7FF),
                gap: true,
            }),
            &mut out,
        )
        .unwrap();
    assert_eq!(a.get(0), Some(&ExtractorOutput::Discarded { octets: 7 }));
}

#[test]
fn the_protocol_trait_has_one_implementation_per_protocol() {
    // req: LNK-001
    assert_eq!(Tm::ID, LinkProtocol::Tm);
    assert_eq!(Tc::ID, LinkProtocol::Tc);
    assert_eq!(Aos::ID, LinkProtocol::Aos);
    assert_eq!(Uslp::ID, LinkProtocol::Uslp);
    assert!(<Tm as Protocol>::Vc::try_from(8).is_err());
    assert!(<Aos as Protocol>::Vc::try_from(63).is_ok());
    assert!(<Tc as Protocol>::Vc::try_from(64).is_err());
    // The pointer values of docs/04-engines.md §2.1.
    let aos5 = AosParams::default();
    let aos4 = AosParams {
        edition: AosEdition::Issue4Compatible,
    };
    let uslp = UslpParams::new(Upid::Packets, MapId::<4>::new(3).unwrap(), Qos::Expedited);
    assert_eq!(Tm::encode_pointer(&TmParams, NoStart).unwrap(), 0x7FF);
    assert_eq!(Tm::encode_pointer(&TmParams, OnlyIdle).unwrap(), 0x7FE);
    assert_eq!(Tm::encode_pointer(&TmParams, StartsAt(5)).unwrap(), 5);
    assert!(Tm::encode_pointer(&TmParams, StartsAt(0x7FE)).is_err());
    assert!(Tm::encode_pointer(&TmParams, StartsAt(usize::MAX)).is_err());
    assert_eq!(Aos::encode_pointer(&aos5, NoStart).unwrap(), 0xFFFF);
    assert_eq!(Aos::encode_pointer(&aos5, OnlyIdle).unwrap(), 0xFFFE);
    assert_eq!(Aos::encode_pointer(&aos4, NoStart).unwrap(), 0x7FF);
    assert_eq!(Aos::encode_pointer(&aos4, OnlyIdle).unwrap(), 0x7FE);
    assert_eq!(Aos::encode_pointer(&aos4, StartsAt(9)).unwrap(), 9);
    assert!(Aos::encode_pointer(&aos4, StartsAt(0x7FE)).is_err());
    assert!(Aos::encode_pointer(&aos5, StartsAt(usize::MAX)).is_err());
    assert_eq!(Uslp::encode_pointer(&uslp, NoStart).unwrap(), 0xFFFF);
    assert_eq!(Uslp::encode_pointer(&uslp, StartsAt(4)).unwrap(), 4);
    assert!(Uslp::encode_pointer(&uslp, OnlyIdle).is_err());
    assert!(Uslp::encode_pointer(&uslp, StartsAt(0xFFFF)).is_err());
    assert!(Uslp::encode_pointer(&uslp, StartsAt(usize::MAX)).is_err());
    assert_eq!(
        (
            Tm::max_zone_len(&TmParams),
            Aos::max_zone_len(&aos5),
            Aos::max_zone_len(&aos4),
            Uslp::max_zone_len(&uslp)
        ),
        (0x7FE, 0xFFFE, 0x7FE, 0xFFFF)
    );
    assert_eq!(Uslp::route(&uslp).unwrap().map.get(), 3);
    assert!(Tm::route(&TmParams).is_none() && Aos::route(&aos5).is_none());
    let mut h = [0u8; 3];
    Uslp::write_header(&uslp, 4, &mut h);
    assert_eq!(h, [0x00, 0x00, 0x04]);
    let mut h = [0u8; 2];
    Aos::write_header(&aos4, 0x7FE, &mut h);
    assert_eq!(h, [0x07, 0xFE]);
    Tm::write_header(&TmParams, 0, &mut []);
}

#[test]
fn the_codec_errors_of_the_data_fields_pass_through() {
    // req: LNK-060, LNK-064
    let aos5 = AosParams::default();
    assert_eq!(
        Aos::parse(&aos5, None, &[0xFF, 0xFE, 1, 2])
            .unwrap()
            .pointer,
        Pointer::OnlyIdle
    );
    assert!(Aos::parse(&aos5, None, &[0xFF, 0xFF]).is_err());
    assert!(Aos::parse(&aos5, None, &[0x00, 0x09, 1, 2]).is_err());
    let uslp = UslpParams::new(
        Upid::Packets,
        MapId::<4>::new(0).unwrap(),
        Qos::SequenceControlled,
    );
    let ok = Uslp::parse(&uslp, None, &[0x00, 0x00, 0x01, 1, 2]).unwrap();
    assert_eq!((ok.pointer, ok.zone), (Pointer::StartsAt(1), &[1u8, 2][..]));
    assert_eq!(
        Uslp::parse(&uslp, None, &[0x00, 0xFF, 0xFF, 1])
            .unwrap()
            .pointer,
        Pointer::NoStart
    );
    // Rule 001 on a UPID other than Only Idle Data is not the extractor's.
    assert_eq!(
        Uslp::parse(&uslp, None, &[0x20, 0x00, 0x00, 1])
            .unwrap_err()
            .field(),
        Some(field::TFDZ_CONSTRUCTION_RULE)
    );
    // An OID frame (rule 001, UPID 31) is idle data whatever the rule.
    assert_eq!(
        Uslp::parse(&uslp, None, &[0x3F, 0x00, 0x17, 1, 2])
            .unwrap()
            .pointer,
        Pointer::OnlyIdle
    );
    for short in [
        &[][..],
        &[0x00],
        &[0x00, 0x00, 0x00],
        &[0x00, 0x00, 0x00, 0x00][..0],
    ] {
        assert!(Uslp::parse(&uslp, None, short).is_err());
    }
    assert_eq!(
        Uslp::parse(&uslp, None, &[0x00, 0x00, 0x05, 1, 2])
            .unwrap_err()
            .kind(),
        ErrorKind::Malformed
    );
}

#[test]
fn configurations_report_what_they_were_built_with() {
    // req: LNK-010, LNK-012, LNK-013, LNK-014, LNK-061, LNK-063, API-041
    let g = tm_config(24)
        .with_idle(IdlePolicy::IdleField, IdlePacketKind::SpacePacket, 0x5A)
        .unwrap()
        .with_release_deadline(Duration::from_micros(7));
    assert_eq!(g.vc().get(), 1);
    assert_eq!((g.field_len(), g.max_packet_len()), (24, 64));
    assert_eq!(g.valid_pvns(), ValidPvns::BOTH);
    assert_eq!(g.release_deadline(), Some(Duration::from_micros(7)));
    assert_eq!(g.idle_policy(), IdlePolicy::IdleField);
    assert_eq!(g.idle_packet_kind(), IdlePacketKind::SpacePacket);
    assert_eq!(g.idle_pattern(), 0x5A);
    let e = ext_config().with_delivery(true, true);
    assert_eq!(e.valid_pvns(), ValidPvns::BOTH);
    assert_eq!(e.max_packet_len(), 40);
    assert!(e.deliver_incomplete() && e.deliver_idle());
    assert!(!ext_config().deliver_incomplete() && !ext_config().deliver_idle());
    let x = TmExt::new(e).unwrap();
    assert_eq!(*x.config(), e);
}

#[test]
fn an_idle_field_never_splits_a_packet() {
    // req: LNK-014, LNK-011
    let cfg = tm_config(24)
        .with_idle(IdlePolicy::IdleField, IdlePacketKind::SpacePacket, 0)
        .unwrap();
    let mut g = TmGen::new(cfg).unwrap();
    let mut out = [0u8; 24];
    // A packet of 36 octets: 24 in the first field, 12 queued. After the first Release the next
    // field starts with the 12 octets that continue the packet, so no idle field may come
    // before it (132.0 §4.2.2.2): Release gives nothing until that field is released.
    g.step(GeneratorInput::SubmitPacket(&spp(36, 5)), T0, &mut out)
        .unwrap();
    let first = g.step(GeneratorInput::Release, T0, &mut out).unwrap();
    assert!(matches!(first.get(0), Some(GeneratorOutput::DataField(_))));
    assert!(
        g.step(GeneratorInput::Release, T0, &mut out)
            .unwrap()
            .is_empty()
    );
    assert_eq!(g.status().idle_fields.get(), 0);
    // A second packet starts after the continuation: still a continuation first.
    g.step(GeneratorInput::SubmitPacket(&spp(8, 6)), T0, &mut out)
        .unwrap();
    assert!(
        g.step(GeneratorInput::Release, T0, &mut out)
            .unwrap()
            .is_empty()
    );
    // Without a deadline the field is released when full or never; here a packet fills it.
    g.step(GeneratorInput::SubmitPacket(&spp(24, 7)), T0, &mut out)
        .unwrap();
    let second = g.step(GeneratorInput::Release, T0, &mut out).unwrap();
    let Some(GeneratorOutput::DataField(f)) = second.get(0) else {
        panic!()
    };
    // 12 octets of continuation, 8 of the second packet, and 4 of the third: the pointer is 12.
    assert_eq!(f.pointer, 12);
    // The next field starts with the rest of the third packet: a continuation again.
    assert!(
        g.step(GeneratorInput::Release, T0, &mut out)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_extractor_resynchronises_in_the_cases_the_vectors_leave_open() {
    // req: LNK-060, LNK-061
    let mut out = [0u8; 100];
    // A packet that ends exactly at the end of a field, then a field whose pointer is not 0:
    // nothing is in progress, so the octets before the pointer disagree with it (FHP wins).
    let mut e = TmExt::new(ext_config()).unwrap();
    let a = e.step(field_in(&spp(24, 1), 0), &mut out).unwrap();
    assert_eq!(a.len(), 1);
    let mut next = vec![0xAA; 3];
    next.extend_from_slice(&spp(21, 2));
    let a = e.step(field_in(&next, 3), &mut out).unwrap();
    assert_eq!(
        a.get(0),
        Some(&ExtractorOutput::Error(ExtractionError::LengthInconsistent))
    );
    assert!(matches!(
        a.get(1),
        Some(ExtractorOutput::Packet {
            incomplete: false,
            ..
        })
    ));
    // A gap with no packet in progress discards nothing.
    let mut e = TmExt::new(ext_config()).unwrap();
    let gap = ExtractorInput::DataField(FieldIn {
        field: &spp(24, 3),
        header_pointer: Some(0),
        gap: true,
    });
    let a = e.step(gap, &mut out).unwrap();
    assert!(matches!(a.get(0), Some(ExtractorOutput::Packet { .. })));
    assert_eq!(e.status().discarded_octets.get(), 0);
    // A partial packet with a known length is delivered incomplete on a gap.
    let mut e = TmExt::new(ext_config().with_delivery(true, false)).unwrap();
    let mut f = spp(10, 1);
    f.extend_from_slice(&spp(30, 2)[..14]);
    e.step(field_in(&f, 0), &mut out).unwrap();
    let gap = ExtractorInput::DataField(FieldIn {
        field: &[0; 24],
        header_pointer: Some(0x7FF),
        gap: true,
    });
    let a = e.step(gap, &mut out).unwrap();
    assert!(matches!(
        a.get(0),
        Some(ExtractorOutput::Packet {
            incomplete: true,
            pvn: 0,
            ..
        })
    ));
    assert_eq!(
        (
            e.status().incomplete_out.get(),
            e.status().packets_out.get()
        ),
        (1, 2)
    );
}
