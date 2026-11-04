//! Round trip of the `PacketGenerator` and the `PacketExtractor` (VER-023, LNK-011…LNK-013,
//! LNK-060, LNK-061, LNK-063, LNK-064): the packets a generator puts into data fields come out
//! of an extractor as they went in, in order, with no error, whatever the field length, the
//! release points, and the idle policy.
//!
//! The cases the M4 plan (§5) names are drawn on purpose: a packet header split across fields
//! (field lengths below 12 split a 6-octet header most of the time), a packet that ends exactly
//! at the end of a field (packet lengths that are multiples of the field length), and idle fill
//! shorter than a packet header (a field released with 1 to 6 octets left, which a Space Packet
//! idle packet spills into the next field and an Encapsulation Idle Packet fills exactly).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "tests fail loudly by design"
)]

#[path = "common/mod.rs"]
mod common;

use aldis_aos::AosEdition;
use aldis_core::ids::MapId;
use aldis_core::time::{Duration, Instant};
use aldis_link::common::{IdlePacketKind, IdlePolicy, Qos, ValidPvns};
use aldis_link::extractor::{
    ExtractorInput, ExtractorOutput, FieldIn, PacketExtractor, PacketExtractorConfig, PacketRun,
};
use aldis_link::generator::{
    GeneratorInput, GeneratorOutput, PacketGenerator, PacketGeneratorConfig,
};
use aldis_link::protocol::{Aos, AosParams, PacketProtocol, Tm, TmParams, Uslp, UslpParams};
use aldis_testkit::strategies;
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_uslp::Upid;

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/packet_roundtrip.txt"
);

/// The data field capacity, packet queue, and assembly buffer of the engines under test.
const D: usize = 80;
const Q: usize = 256;
const M: usize = 96;
const MAX_PACKET: usize = 64;
const DEADLINE: Duration = Duration::from_micros(10);

/// A Space Packet of `total` octets (7 to 64) that is not an idle packet.
fn space_packet(total: usize, apid: u16, fill: u8) -> Vec<u8> {
    let apid = apid % 2047;
    let mut p = vec![
        u8::try_from(apid >> 8).unwrap(),
        u8::try_from(apid & 0xFF).unwrap(),
        0xC0,
        0x00,
    ];
    p.extend_from_slice(&u16::try_from(total - 7).unwrap().to_be_bytes());
    p.resize(total, fill);
    p
}

/// An Encapsulation Packet of `total` octets (3 to 64), a 2-octet header and a protocol ID that
/// is not the idle one; or, when `four` and `total` is at least 4, a 4-octet header.
fn encapsulation_packet(total: usize, protocol: u8, four: bool, fill: u8) -> Vec<u8> {
    let epi = 1 + protocol % 5;
    let mut p = if four && total >= 5 {
        let mut p = vec![0xE0 | (epi << 2) | 0b10, 0x00];
        p.extend_from_slice(&u16::try_from(total).unwrap().to_be_bytes());
        p
    } else {
        vec![0xE0 | (epi << 2) | 0b01, u8::try_from(total).unwrap()]
    };
    p.resize(total, fill);
    p
}

#[derive(Debug, Clone)]
enum Op {
    Submit(Vec<u8>),
    /// The release deadline passes and the generator is asked for a field.
    Flush,
}

/// A packet, with lengths that often fill fields exactly.
fn packet(field_zone: usize, pvns: ValidPvns) -> impl Strategy<Value = Vec<u8>> {
    let space_len = prop_oneof![
        3 => 7usize..=MAX_PACKET,
        2 => (1usize..=3).prop_map(move |k| (k * field_zone).clamp(7, MAX_PACKET)),
        1 => Just(7usize),
    ];
    let space = (space_len, any::<u16>(), any::<u8>())
        .prop_map(|(n, apid, fill)| space_packet(n, apid, fill));
    let encapsulation_len = prop_oneof![
        3 => 3usize..=MAX_PACKET,
        2 => (1usize..=3).prop_map(move |k| (k * field_zone).clamp(3, MAX_PACKET)),
    ];
    let encapsulation = (encapsulation_len, any::<u8>(), any::<bool>(), any::<u8>())
        .prop_map(|(n, e, four, fill)| encapsulation_packet(n, e, four, fill));
    match (pvns.space(), pvns.encapsulation()) {
        (true, true) => prop_oneof![space, encapsulation].boxed(),
        (true, false) => space.boxed(),
        _ => encapsulation.boxed(),
    }
}

fn ops(field_zone: usize, pvns: ValidPvns) -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(
        prop_oneof![
            5 => packet(field_zone, pvns).prop_map(Op::Submit),
            2 => Just(Op::Flush),
        ],
        1..40,
    )
}

fn pvns() -> impl Strategy<Value = ValidPvns> {
    prop_oneof![
        2 => Just(ValidPvns::BOTH),
        1 => Just(ValidPvns::SPACE),
        1 => Just(ValidPvns::ENCAPSULATION),
    ]
}

/// The outcome of one round trip: what went in, and what came out.
struct Trip {
    sent: Vec<Vec<u8>>,
    received: Vec<Vec<u8>>,
    problems: Vec<String>,
}

/// Feeds the field of a `DataField` action to the extractor, as the `FrameReceiver` would.
fn extract<P: PacketProtocol, const EM: usize, const ED: usize>(
    extractor: &mut PacketExtractor<P, EM, ED>,
    field: &[u8],
    pointer: u16,
    trip: &mut Trip,
) {
    let mut area = vec![0u8; EM + ED];
    let input = ExtractorInput::DataField(FieldIn {
        field,
        header_pointer: Some(pointer),
        gap: false,
    });
    match extractor.step(input, &mut area) {
        Ok(actions) => {
            for a in actions.iter() {
                #[expect(
                    clippy::wildcard_enum_match_arm,
                    reason = "a test collects the Packets outputs; any other output is a problem"
                )]
                match a {
                    ExtractorOutput::Packets { run, .. } => trip
                        .received
                        .extend(PacketRun::new(*run, &area).unwrap().map(<[u8]>::to_vec)),
                    other => trip.problems.push(format!("{other:?}")),
                }
            }
        }
        Err(e) => trip.problems.push(format!("{e:?}")),
    }
}

/// One step of the sending side: the release deadline has passed, and the generator is asked for
/// a field, which the extractor takes. Returns whether a field came out.
fn flush<P: PacketProtocol>(
    generator: &mut PacketGenerator<P, D, Q>,
    extractor: &mut PacketExtractor<P, M, D>,
    now: &mut u64,
    trip: &mut Trip,
) -> bool {
    let mut area = vec![0u8; D];
    *now += 100;
    let at = Instant::from_micros(*now);
    let _ = generator
        .step(GeneratorInput::Timeout, at, &mut area)
        .unwrap();
    let actions = generator
        .step(GeneratorInput::Release, at, &mut area)
        .unwrap();
    let mut any = false;
    for a in actions.iter() {
        if let GeneratorOutput::DataField(f) = a {
            any = true;
            let field = f.field.slice(&area).unwrap().to_vec();
            extract(extractor, &field, f.pointer, trip);
        }
    }
    any
}

/// Runs `ops` through a generator of `field_len` octets and an extractor, then flushes.
fn round_trip<P: common::Proto>(
    params: P::Params,
    field_len: usize,
    pvns: ValidPvns,
    policy: IdlePolicy,
    kind: IdlePacketKind,
    pattern: u8,
    ops: &[Op],
) -> Trip {
    let config = PacketGeneratorConfig::<P>::new(
        P::Vc::try_from(1).unwrap(),
        params,
        field_len,
        pvns,
        MAX_PACKET,
    )
    .unwrap()
    .with_idle(policy, kind, pattern)
    .unwrap()
    .with_release_deadline(DEADLINE);
    let mut generator = PacketGenerator::<P, D, Q>::new(config).unwrap();
    let ext_config = PacketExtractorConfig::<P>::new(params, pvns, MAX_PACKET).unwrap();
    let mut extractor = PacketExtractor::<P, M, D>::new(ext_config).unwrap();
    let mut trip = Trip {
        sent: Vec::new(),
        received: Vec::new(),
        problems: Vec::new(),
    };
    let mut now = 0u64;
    let mut area = vec![0u8; D];
    for op in ops {
        match op {
            Op::Submit(packet) => {
                // WouldBlock: release until the packet fits (a framework would wait).
                loop {
                    let at = Instant::from_micros(now);
                    match generator.step(GeneratorInput::SubmitPacket(packet), at, &mut area) {
                        Ok(_) => {
                            trip.sent.push(packet.clone());
                            break;
                        }
                        Err(e) if e.kind() == aldis_core::ErrorKind::WouldBlock => {
                            assert!(flush(&mut generator, &mut extractor, &mut now, &mut trip));
                        }
                        Err(e) => panic!("{e:?} {packet:02X?}"),
                    }
                }
            }
            Op::Flush => {
                flush(&mut generator, &mut extractor, &mut now, &mut trip);
            }
        }
    }
    // Drain: the last fields are completed with idle packets. With the IdleField policy a
    // generator never runs dry, so the drain stops after the packets are out.
    for _ in 0..(Q + D) {
        let more = flush(&mut generator, &mut extractor, &mut now, &mut trip);
        let drained =
            generator.status().queue.current() == 0 && generator.status().field.current() == 0;
        if !more || (policy == IdlePolicy::IdleField && drained) {
            break;
        }
    }
    trip
}

/// Checks a trip: the packets that went in came out, in order, and nothing else was reported.
fn check(trip: &Trip) -> Result<(), TestCaseError> {
    prop_assert!(trip.problems.is_empty(), "{:?}", trip.problems);
    prop_assert_eq!(&trip.sent, &trip.received);
    Ok(())
}

/// A field length in `lengths`, the valid set, and the operations drawn for them; `header` is
/// the octets of the data field header (the packet zone is the rest).
fn scenario(
    lengths: core::ops::RangeInclusive<usize>,
    header: usize,
) -> impl Strategy<Value = (usize, ValidPvns, Vec<Op>)> {
    (lengths, pvns()).prop_flat_map(move |(field_len, p)| {
        (
            Just(field_len),
            Just(p),
            ops(field_len.saturating_sub(header), p),
        )
    })
}

/// Space Packet idle packets, or Encapsulation Idle Packets where the set allows them.
const fn idle_kind(p: ValidPvns, prefer_encapsulation: bool) -> IdlePacketKind {
    if p.space() && !(prefer_encapsulation && p.encapsulation()) {
        IdlePacketKind::SpacePacket
    } else {
        IdlePacketKind::Encapsulation
    }
}

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn tm_round_trip(
        (field_len, p, list) in scenario(7..=40, 0),
        eip in any::<bool>(),
        idle_field in any::<bool>(),
        pattern in any::<u8>(),
    ) {
        // req: VER-023, LNK-011, LNK-012, LNK-013, LNK-014, LNK-060, LNK-061, LNK-063
        let policy = if idle_field { IdlePolicy::IdleField } else { IdlePolicy::Oid };
        check(&round_trip::<Tm>(TmParams, field_len, p, policy, idle_kind(p, eip), pattern, &list))?;
    }

    #[test]
    fn aos_round_trip(
        (field_len, p, list) in scenario(9..=42, 2),
        issue4 in any::<bool>(),
        eip in any::<bool>(),
        pattern in any::<u8>(),
    ) {
        // req: VER-023, LNK-011, LNK-013, LNK-060, LNK-064
        let params = AosParams {
            edition: if issue4 { AosEdition::Issue4Compatible } else { AosEdition::Issue5 },
        };
        check(&round_trip::<Aos>(
            params, field_len, p, IdlePolicy::Oid, idle_kind(p, eip), pattern, &list,
        ))?;
    }

    #[test]
    fn uslp_round_trip(
        (field_len, p, list) in scenario(10..=43, 3),
        pattern in any::<u8>(),
    ) {
        // req: VER-023, LNK-011, LNK-013, LNK-060, LNK-063, LNK-064
        let params = UslpParams::new(Upid::Packets, MapId::<4>::new(2).unwrap(), Qos::Expedited)
            .unwrap();
        check(&round_trip::<Uslp>(
            params, field_len, p, IdlePolicy::Oid, IdlePacketKind::Encapsulation, pattern, &list,
        ))?;
    }

    /// Whatever the fields, the gap flags, and the pointers, one input of the extractor never
    /// fails with an internal error, which is what `Actions::push` returns past
    /// `EXTRACTOR_MAX_ACTIONS`, so the bound holds and no push fails after the state has changed
    /// (LNK-074, API-057, API-058).
    #[test]
    fn extractor_actions_stay_within_the_bound(
        fields in prop::collection::vec(
            (
                strategies::octets(1..=24),
                prop_oneof![0u16..24, Just(0x7FFu16), Just(0x7FEu16)],
                any::<bool>(),
            ),
            1..30,
        ),
        deliver_incomplete in any::<bool>(),
        deliver_idle in any::<bool>(),
    ) {
        // req: LNK-074, API-057, API-058, VER-023
        let config = PacketExtractorConfig::<Tm>::new(TmParams, ValidPvns::BOTH, 40)
            .unwrap()
            .with_delivery(deliver_incomplete, deliver_idle);
        let mut extractor = PacketExtractor::<Tm, M, 24>::new(config).unwrap();
        let mut area = vec![0u8; M + 24];
        for (field, pointer, gap) in &fields {
            let input = ExtractorInput::DataField(FieldIn {
                field,
                header_pointer: Some(*pointer),
                gap: *gap,
            });
            match extractor.step(input, &mut area) {
                Ok(_) => {}
                Err(e) => prop_assert_ne!(e.kind(), aldis_core::ErrorKind::InternalError),
            }
        }
    }
}
