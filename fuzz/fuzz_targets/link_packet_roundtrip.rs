//! The `PacketGenerator` into the `PacketExtractor` (VER-022, LNK-011…LNK-013, LNK-060, LNK-061,
//! LNK-063, LNK-064, LNK-074, API-057, API-058): the packets a generator puts into data fields of
//! a TM, AOS, or USLP channel come out of an extractor as they went in, in order, with no error,
//! whatever the field length, the release points, and the idle policy. The extractor is also fed
//! arbitrary fields, pointers, and gap flags: it never reports an internal error.
#![no_main]

// req: VER-022, VER-023, LNK-011, LNK-012, LNK-013, LNK-060, LNK-061, LNK-063, LNK-064, LNK-074, API-057, API-058

use aldis_aos::AosEdition;
use aldis_core::ErrorKind;
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
use aldis_uslp::Upid;
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

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

/// An Encapsulation Packet of `total` octets (3 to 64), with a 2-octet header or, when `four`, a
/// 4-octet header; the protocol ID is not the idle one.
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

enum Op {
    Submit(Vec<u8>),
    Flush,
}

fn packet(u: &mut Unstructured<'_>, field_zone: usize, pvns: ValidPvns) -> Result<Vec<u8>> {
    let sized = |u: &mut Unstructured<'_>, min: usize| -> Result<usize> {
        Ok(if u.int_in_range(0..=2)? == 0 {
            (u.int_in_range(1..=3)? * field_zone).clamp(min, MAX_PACKET)
        } else {
            u.int_in_range(min..=MAX_PACKET)?
        })
    };
    let space = pvns.space() && (!pvns.encapsulation() || u.arbitrary()?);
    if space {
        let n = sized(u, 7)?;
        Ok(space_packet(n, u.arbitrary()?, u.arbitrary()?))
    } else {
        let n = sized(u, 3)?;
        Ok(encapsulation_packet(
            n,
            u.arbitrary()?,
            u.arbitrary()?,
            u.arbitrary()?,
        ))
    }
}

struct Trip {
    sent: Vec<Vec<u8>>,
    received: Vec<Vec<u8>>,
}

/// Feeds a field to the extractor, as the `FrameReceiver` would.
fn extract<P: PacketProtocol>(
    extractor: &mut PacketExtractor<P, M, D>,
    field: &[u8],
    pointer: u16,
    trip: &mut Trip,
) {
    let mut area = vec![0u8; M + D];
    let input = ExtractorInput::DataField(FieldIn {
        field,
        header_pointer: Some(pointer),
        gap: false,
    });
    let actions = extractor.step(input, &mut area).unwrap();
    for a in actions.iter() {
        let ExtractorOutput::Packets { run, .. } = a else {
            panic!("{a:?}");
        };
        trip.received
            .extend(PacketRun::new(*run, &area).unwrap().map(<[u8]>::to_vec));
    }
}

/// The release deadline passes and the generator is asked for a field, which the extractor
/// takes. Returns whether a field came out.
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

fn round_trip<P: PacketProtocol>(
    u: &mut Unstructured<'_>,
    params: P::Params,
    header: usize,
    lengths: (usize, usize),
    policy: IdlePolicy,
    prefer_encapsulation: Option<bool>,
) -> Result<()> {
    let field_len = u.int_in_range(lengths.0..=lengths.1)?;
    let pvns = match u.int_in_range(0..=3)? {
        0 | 1 => ValidPvns::BOTH,
        2 => ValidPvns::SPACE,
        _ => ValidPvns::ENCAPSULATION,
    };
    // An idle packet of the kind the set allows; USLP always fills with an Encapsulation one.
    let kind = match prefer_encapsulation {
        Some(prefer) if pvns.space() && !(prefer && pvns.encapsulation()) => {
            IdlePacketKind::SpacePacket
        }
        _ => IdlePacketKind::Encapsulation,
    };
    let pattern = u.arbitrary()?;
    let mut ops = Vec::new();
    for _ in 0..u.int_in_range(1..=40)? {
        if u.int_in_range(0..=6)? < 2 {
            ops.push(Op::Flush);
        } else {
            ops.push(Op::Submit(packet(
                u,
                field_len.saturating_sub(header),
                pvns,
            )?));
        }
    }
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
    };
    let mut now = 0u64;
    let mut area = vec![0u8; D];
    for op in &ops {
        match op {
            Op::Submit(packet) => {
                // bound: each WouldBlock is followed by a release that empties a field; the
                // queue holds more than one field.
                for _ in 0..=Q {
                    let at = Instant::from_micros(now);
                    match generator.step(GeneratorInput::SubmitPacket(packet), at, &mut area) {
                        Ok(_) => {
                            trip.sent.push(packet.clone());
                            break;
                        }
                        Err(e) if e.kind() == ErrorKind::WouldBlock => {
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
    for _ in 0..(Q + D) {
        let more = flush(&mut generator, &mut extractor, &mut now, &mut trip);
        let drained =
            generator.status().queue.current() == 0 && generator.status().field.current() == 0;
        if !more || (policy == IdlePolicy::IdleField && drained) {
            break;
        }
    }
    assert_eq!(trip.sent, trip.received);
    Ok(())
}

/// Arbitrary fields, pointers, and gaps into an extractor.
fn extractor_inputs(u: &mut Unstructured<'_>) -> Result<()> {
    let config = PacketExtractorConfig::<Tm>::new(TmParams, ValidPvns::BOTH, 40)
        .unwrap()
        .with_delivery(u.arbitrary()?, u.arbitrary()?);
    let mut extractor = PacketExtractor::<Tm, M, 24>::new(config).unwrap();
    let mut area = vec![0u8; M + 24];
    for _ in 0..u.int_in_range(1..=30)? {
        let n = u.int_in_range(1..=24)?.min(u.len());
        let field = u.bytes(n)?.to_vec();
        let pointer = match u.int_in_range(0..=3)? {
            0 => 0x7FF,
            1 => 0x7FE,
            _ => u.int_in_range(0..=24)?,
        };
        let input = ExtractorInput::DataField(FieldIn {
            field: &field,
            header_pointer: Some(pointer),
            gap: u.arbitrary()?,
        });
        match extractor.step(input, &mut area) {
            Ok(_) => {}
            Err(e) => {
                assert_ne!(e.kind(), ErrorKind::InternalError);
            }
        }
    }
    Ok(())
}

fn run(u: &mut Unstructured<'_>) -> Result<()> {
    let idle_field = u.arbitrary()?;
    let policy = if idle_field {
        IdlePolicy::IdleField
    } else {
        IdlePolicy::Oid
    };
    let prefer: Option<bool> = Some(u.arbitrary()?);
    match u.int_in_range(0..=3)? {
        0 => round_trip::<Tm>(u, TmParams, 0, (7, 40), policy, prefer),
        1 => {
            let params = AosParams {
                edition: if u.arbitrary()? {
                    AosEdition::Issue4Compatible
                } else {
                    AosEdition::Issue5
                },
            };
            round_trip::<Aos>(u, params, 2, (9, 42), IdlePolicy::Oid, prefer)
        }
        2 => {
            let params =
                UslpParams::new(Upid::Packets, MapId::<4>::new(2).unwrap(), Qos::Expedited)
                    .unwrap();
            round_trip::<Uslp>(u, params, 3, (10, 43), IdlePolicy::Oid, None)
        }
        _ => extractor_inputs(u),
    }
}

fuzz_target!(|data: &[u8]| {
    // An input too short for a scenario is skipped.
    let _ = run(&mut Unstructured::new(data));
});
