//! Calls every public `aldis-link` API of the packet engines (VER-010; M4 WP-3): the
//! `PacketGenerator` and the `PacketExtractor` for TM, AOS (both editions), and USLP, with their
//! configurations, inputs, refusals, and status. Inputs pass through `black_box`. The
//! `link/` vector files are engine scenarios, which the host suites run; the group embeds no
//! case (as `cop1/` does).

use core::hint::black_box as bb;

use aldis_aos::AosEdition;
use aldis_core::ErrorKind;
use aldis_core::ids::MapId;
use aldis_core::time::{Duration, Instant};
use aldis_link::common::{Counter, IdlePacketKind, IdlePolicy, Occupancy, Qos, ValidPvns};
use aldis_link::extractor::{
    ExtractionError, ExtractorInput, ExtractorOutput, FieldIn, MIN_ASSEMBLY, PacketExtractor,
    PacketExtractorConfig, extractor_max_actions,
};
use aldis_link::generator::{
    GeneratorInput, GeneratorOutput, PACKET_GENERATOR_MAX_ACTIONS, PacketGenerator,
    PacketGeneratorConfig,
};
use aldis_link::protocol::{
    Aos, AosParams, IdleField, LinkProtocol, PacketProtocol, Pointer, Protocol, Tc, Tm, TmParams,
    Uslp, UslpParams,
};
use aldis_uslp::Upid;

use crate::exercise::Tally;
use crate::vectors::{Case, Fail, Run};

/// The data field, the packet queue, the assembly buffer, and the packet zone of the engines.
const D: usize = 40;
const Q: usize = 96;
const M: usize = 64;
const N: usize = extractor_max_actions(D, 1);

/// A packet in a fixed buffer.
#[derive(Clone, Copy)]
struct Packet {
    octets: [u8; 48],
    len: usize,
}

impl Packet {
    fn octets(&self) -> &[u8] {
        self.octets.get(..self.len).unwrap_or_default()
    }
}

/// A Space Packet of `total` octets (at least 7), APID 5, count 0, data `fill`.
fn space(total: usize, fill: u8) -> Packet {
    let mut octets = [fill; 48];
    let length = u16::try_from(total.saturating_sub(7))
        .unwrap_or(0)
        .to_be_bytes();
    for (d, s) in octets
        .iter_mut()
        .zip([0x00, 0x05, 0xC0, 0x00, length[0], length[1]])
    {
        *d = s;
    }
    Packet { octets, len: total }
}

/// An Encapsulation Packet of `total` octets (at least 3) with a 2-octet header.
fn encapsulation(total: usize, fill: u8) -> Packet {
    let mut octets = [fill; 48];
    let length = u8::try_from(total).unwrap_or(0);
    for (d, s) in octets.iter_mut().zip([0xF5, length]) {
        *d = s;
    }
    Packet { octets, len: total }
}

/// Generator to extractor over `P`: packets that span fields, a field released at the deadline
/// and completed with idle packets, the refusals, `Reconfigure`, and the status.
fn trip<P: PacketProtocol>(
    t: &mut Tally,
    params: P::Params,
    field_len: usize,
    kind: IdlePacketKind,
    policy: IdlePolicy,
) {
    let Ok(vc) = P::Vc::try_from(bb(1)) else {
        t.check(false);
        return;
    };
    let Ok(base) =
        PacketGeneratorConfig::<P>::new(vc, params, bb(field_len), bb(ValidPvns::BOTH), bb(48))
    else {
        t.check(false);
        return;
    };
    let Ok(config) = base.with_idle(bb(policy), bb(kind), bb(0xA5)) else {
        t.check(false);
        return;
    };
    let config = config.with_release_deadline(Duration::from_micros(bb(100)));
    t.check(config.vc() == vc && config.field_len() == field_len);
    t.check(config.valid_pvns() == ValidPvns::BOTH && config.max_packet_len() == 48);
    t.check(config.release_deadline() == Some(Duration::from_micros(100)));
    t.check(config.idle_policy() == policy && config.idle_pattern() == 0xA5);
    t.check(config.idle_packet_kind() == P::IDLE_KIND.unwrap_or(kind));
    let Ok(mut generator) = PacketGenerator::<P, D, Q>::new(config) else {
        t.check(false);
        return;
    };
    t.check(generator.config() == &config && generator.next_deadline().is_none());
    let Ok(ext_config) = PacketExtractorConfig::<P>::new(params, ValidPvns::BOTH, bb(48)) else {
        t.check(false);
        return;
    };
    let ext_config = ext_config.with_delivery(bb(true), bb(false));
    t.check(ext_config.valid_pvns() == ValidPvns::BOTH && ext_config.max_packet_len() == 48);
    t.check(ext_config.deliver_incomplete() && !ext_config.deliver_idle());
    t.check(ext_config.min_packet_len() == 3);
    let Ok(mut extractor) = PacketExtractor::<P, M, D, N>::new(ext_config) else {
        t.check(false);
        return;
    };
    t.check(extractor.config() == &ext_config);

    let mut out = [0u8; D];
    let packets = [space(bb(30), 1), encapsulation(bb(12), 2), space(bb(7), 3)];
    for (i, p) in packets.iter().enumerate() {
        let at = Instant::from_micros(u64::try_from(i).unwrap_or(0));
        t.check(
            generator
                .step(GeneratorInput::SubmitPacket(bb(p.octets())), at, &mut out)
                .is_ok(),
        );
    }
    // The first field is full, so no deadline runs.
    t.check(generator.next_deadline().is_none());
    flow(t, &mut generator, &mut extractor, &packets, params);

    refusals(t, &mut generator, config);
    extractor_refusals(t, &mut extractor, ext_config);
}

/// Releases fields until the packets are out, and feeds each to the extractor.
fn flow<P: PacketProtocol>(
    t: &mut Tally,
    generator: &mut PacketGenerator<P, D, Q>,
    extractor: &mut PacketExtractor<P, M, D, N>,
    packets: &[Packet; 3],
    params: P::Params,
) {
    let mut out = [0u8; D];
    let mut area = [0u8; M + D];
    let mut delivered = 0usize;
    let mut fields = 0usize;
    for round in 0..6_u64 {
        let at = Instant::from_micros(bb(1_000_u64.saturating_add(round.saturating_mul(1_000))));
        t.check(
            generator
                .step(GeneratorInput::Timeout, at, &mut out)
                .is_ok(),
        );
        let Ok(actions) = generator.step(GeneratorInput::Release, at, &mut out) else {
            t.check(false);
            return;
        };
        for action in actions.iter() {
            let GeneratorOutput::DataField(f) = action else {
                continue;
            };
            fields = fields.saturating_add(1);
            let Some(field) = f.field.slice(&out) else {
                t.check(false);
                continue;
            };
            t.check(f.vc == 1 && f.route == P::route(&params));
            let input = ExtractorInput::DataField(FieldIn {
                field: bb(field),
                header_pointer: Some(f.pointer),
                gap: false,
            });
            let Ok(got) = extractor.step(input, &mut area) else {
                t.check(false);
                continue;
            };
            for output in got.iter() {
                if let ExtractorOutput::Packet {
                    pvn,
                    packet,
                    incomplete,
                } = output
                {
                    let same = packets
                        .get(delivered)
                        .is_some_and(|p| packet.slice(&area) == Some(p.octets()));
                    t.check(
                        same && !incomplete
                            && *pvn
                                == packet
                                    .slice(&area)
                                    .map_or(9, |p| p.first().map_or(9, |b| b >> 5)),
                    );
                    delivered = delivered.saturating_add(1);
                }
            }
        }
    }
    t.check(delivered == 3 && fields >= 3);
}

/// The refusals of a generator leave it as it was; `Reconfigure` discards what it holds.
fn refusals<P: PacketProtocol>(
    t: &mut Tally,
    generator: &mut PacketGenerator<P, D, Q>,
    config: PacketGeneratorConfig<P>,
) {
    let mut out = [0u8; D];
    let at = Instant::from_micros(2_000);
    let invalid = space(7, 0);
    let mut wrong_version = invalid;
    if let Some(b) = wrong_version.octets.first_mut() {
        *b = 0x20;
    }
    let mut mismatch = space(8, 0);
    mismatch.len = 9;
    let refused = |g: &mut PacketGenerator<P, D, Q>, p: &[u8], out: &mut [u8; D]| {
        g.step(GeneratorInput::SubmitPacket(bb(p)), at, out)
            .err()
            .map(|e| e.kind())
    };
    t.check(refused(generator, wrong_version.octets(), &mut out) == Some(ErrorKind::InvalidField));
    t.check(refused(generator, mismatch.octets(), &mut out) == Some(ErrorKind::LengthMismatch));
    t.check(refused(generator, &[], &mut out) == Some(ErrorKind::InvalidField));
    let mut many = 0usize;
    // bound: the queue holds at most Q octets, and every accepted packet adds at least 7.
    while many < Q && refused(generator, space(bb(48), 4).octets(), &mut out).is_none() {
        many = many.saturating_add(1);
    }
    t.check(refused(generator, space(bb(48), 4).octets(), &mut out) == Some(ErrorKind::WouldBlock));
    let mut short = [0u8; 4];
    t.check(
        generator
            .step(
                GeneratorInput::Timeout,
                Instant::from_micros(9_000),
                &mut short,
            )
            .is_ok(),
    );
    t.check(
        generator
            .step(
                GeneratorInput::Release,
                Instant::from_micros(9_000),
                &mut short,
            )
            .err()
            .map(|e| e.kind())
            == Some(ErrorKind::BufferTooSmall),
    );
    let s = generator.status();
    t.check(s.packets_in.get() >= 3 && s.would_block.get() >= 1 && s.refused.get() >= 3);
    t.check(s.fields_out.get() >= 3 && s.idle_packets.get() >= 1 && s.idle_fields.get() == 0);
    t.check(s.discarded_octets.get() == 0 && s.queue.high_water() >= s.queue.current());
    t.check(s.field.high_water() >= s.field.current());
    let Ok(held) = generator.step(GeneratorInput::Reconfigure(config), at, &mut out) else {
        t.check(false);
        return;
    };
    t.check(matches!(
        held.get(0),
        Some(GeneratorOutput::Discarded { octets }) if *octets > 0
    ));
    t.check(PACKET_GENERATOR_MAX_ACTIONS == 1 && generator.status().discarded_octets.get() > 0);
}

/// The extractor takes a gap, refuses an area that is too small, counts, and reconfigures.
fn extractor_refusals<P: PacketProtocol>(
    t: &mut Tally,
    extractor: &mut PacketExtractor<P, M, D, N>,
    ext_config: PacketExtractorConfig<P>,
) {
    let mut area = [0u8; M + D];
    let field = [0u8; D];
    let gap = ExtractorInput::DataField(FieldIn {
        field: bb(&field),
        header_pointer: Some(bb(0x7FF)),
        gap: true,
    });
    t.check(extractor.step(gap, &mut area).is_ok());
    let mut tiny = [0u8; 8];
    let input = ExtractorInput::DataField(FieldIn {
        field: bb(&field),
        header_pointer: Some(0x7FF),
        gap: false,
    });
    t.check(
        extractor.step(input, &mut tiny).err().map(|e| e.kind()) == Some(ErrorKind::BufferTooSmall),
    );
    let s = extractor.status();
    t.check(s.fields_in.get() >= 4 && s.packets_out.get() >= 3 && s.gaps.get() == 1);
    t.check(s.assembly.high_water() >= 7 && s.idle_packets.get() <= s.fields_in.get());
    t.check(
        s.incomplete_out.get() == 0
            && s.packet_too_long.get() == 0
            && s.invalid_pvn.get() == 0
            && s.length_inconsistent.get() == 0,
    );
    t.check(
        extractor
            .step(ExtractorInput::Reconfigure(ext_config), &mut area)
            .is_ok(),
    );
    t.check(extractor_max_actions(D, 1) == N && MIN_ASSEMBLY == 8);
}

/// The pointer values and the data field headers of `P` (docs/04-engines.md §2.1).
fn pointers<P: PacketProtocol>(t: &mut Tally, params: P::Params, none: u16, idle: Option<u16>) {
    t.check(P::encode_pointer(&params, Pointer::NoStart).ok() == Some(none));
    t.check(P::encode_pointer(&params, Pointer::OnlyIdle).ok() == idle);
    t.check(P::encode_pointer(&params, Pointer::StartsAt(bb(3))).ok() == Some(3));
    t.check(P::encode_pointer(&params, Pointer::StartsAt(bb(usize::MAX))).is_err());
    t.check(P::max_zone_len(&params) > 1);
    let mut header = [0u8; 3];
    if let Some(h) = header.get_mut(..P::HEADER_LEN) {
        P::write_header(&params, bb(3), h);
    }
    let mut field = [0u8; 8];
    let zone = field.get_mut(P::HEADER_LEN..).unwrap_or_default();
    for (d, s) in zone.iter_mut().zip([9u8, 8, 7]) {
        *d = s;
    }
    for (d, s) in field.iter_mut().zip(header.iter().take(P::HEADER_LEN)) {
        *d = *s;
    }
    let parsed = P::parse(&params, Some(bb(3)), &field);
    t.check(parsed.is_ok_and(|p| p.pointer == Pointer::StartsAt(3)));
    t.check(P::parse(&params, Some(bb(0x7FF)), &[]).is_err() || P::HEADER_LEN == 0);
    t.check(P::DELIMITS_IDLE_EIP == (P::ID == LinkProtocol::Uslp));
    t.check(matches!(
        P::IDLE_FIELD,
        IdleField::Packets | IdleField::Pattern
    ));
}

/// The shared types.
fn common(t: &mut Tally) {
    let mut c = Counter::new();
    c.incr();
    c.add(bb(4));
    t.check(c.get() == 5 && Counter::default() < c);
    let mut o = Occupancy::new();
    o.set(bb(9));
    o.set(bb(2));
    t.check(o.current() == 2 && o.high_water() == 9);
    t.check(Qos::default() == Qos::SequenceControlled && Qos::Expedited != Qos::default());
    t.check(ValidPvns::new(bb(false), bb(false)).is_err());
    t.check(ValidPvns::new(bb(true), bb(false)) == Ok(ValidPvns::SPACE));
    t.check(ValidPvns::from_versions(&[bb(7)]) == Ok(ValidPvns::ENCAPSULATION));
    t.check(ValidPvns::from_versions(&[bb(0), 7]) == Ok(ValidPvns::BOTH));
    t.check(ValidPvns::from_versions(&[bb(1)]).is_err() && ValidPvns::from_versions(&[]).is_err());
    t.check(ValidPvns::BOTH.space() && ValidPvns::BOTH.encapsulation());
    t.check(IdlePacketKind::SpacePacket.is_valid_in(ValidPvns::SPACE));
    t.check(!IdlePacketKind::Encapsulation.is_valid_in(ValidPvns::SPACE));
    t.check(IdlePolicy::default() == IdlePolicy::Oid);
    t.check(Tm::ID == LinkProtocol::Tm && Tc::ID == LinkProtocol::Tc);
    t.check(Aos::ID == LinkProtocol::Aos && Uslp::ID == LinkProtocol::Uslp);
    t.check(<Tm as Protocol>::Vc::try_from(bb(8)).is_err());
    t.check(<Tc as Protocol>::Vc::try_from(bb(63)).is_ok());
    t.check(ExtractionError::PacketTooLong != ExtractionError::InvalidPvn);
}

pub fn run(t: &mut Tally) {
    common(t);
    let uslp =
        MapId::<4>::new(bb(2)).map(|map| UslpParams::new(Upid::Packets, map, Qos::Expedited));
    let aos4 = AosParams {
        edition: AosEdition::Issue4Compatible,
    };
    trip::<Tm>(
        t,
        TmParams,
        24,
        IdlePacketKind::SpacePacket,
        IdlePolicy::Oid,
    );
    trip::<Tm>(
        t,
        TmParams,
        24,
        IdlePacketKind::Encapsulation,
        IdlePolicy::IdleField,
    );
    trip::<Aos>(
        t,
        AosParams::default(),
        26,
        IdlePacketKind::SpacePacket,
        IdlePolicy::Oid,
    );
    trip::<Aos>(
        t,
        aos4,
        26,
        IdlePacketKind::Encapsulation,
        IdlePolicy::IdleField,
    );
    pointers::<Tm>(t, TmParams, 0x7FF, Some(0x7FE));
    pointers::<Aos>(t, AosParams::default(), 0xFFFF, Some(0xFFFE));
    pointers::<Aos>(t, aos4, 0x7FF, Some(0x7FE));
    if let Some(params) = uslp {
        trip::<Uslp>(
            t,
            params,
            27,
            IdlePacketKind::Encapsulation,
            IdlePolicy::Oid,
        );
        pointers::<Uslp>(t, params, 0xFFFF, None);
        t.check(Uslp::route(&params).is_some_and(|r| r.map.get() == 2 && r.qos == Qos::Expedited));
    } else {
        t.check(false);
    }
}

/// The `link/` vector group (CCSDS engine scenarios, M4 WP-2a and WP-2b). The directory holds
/// engine scenarios, which the host suites run, so no case is generated; a case that did
/// arrive fails until it gets a handler.
pub const fn vector(_case: &Case) -> Run {
    Err(Fail::Vector)
}
