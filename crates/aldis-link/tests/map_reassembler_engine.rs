//! Configuration, refusals, bounds, and the choices of the `MapReassembler` that the vectors do
//! not pin (LNK-002, LNK-003, LNK-070…LNK-075, API-058; OI-78): gaps with several SDUs in progress,
//! a gap on an octet stream, a blocked FDU whose packets do not fit, a Last Valid Octet Pointer
//! beyond the zone, a rule of another service, the maximum SDU size.
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    unused_must_use,
    reason = "tests fail loudly by design, and many steps are run for their effect on the state"
)]

use ReassemblyEvent::{GapDiscard, SduTooLong, SequenceViolation, UnknownMap};
use aldis_core::ErrorKind;
use aldis_core::field;
use aldis_core::ids::{MapId, VcId};
use aldis_link::extractor::PacketRun;
use aldis_link::protocol::{Tc, Uslp};
use aldis_link::reassembler::{
    FduIn, MAP_REASSEMBLER_MAX_ACTIONS, MapReassembler, MapReassemblerConfig, ReassemblerInput,
    ReassemblerMap, ReassemblerOutput, ReassemblyEvent, TcReassemblerParams, UslpFdu,
};

const M: usize = 4;
const S: usize = 16;

type TcEngine = MapReassembler<Tc, M, S>;
type UslpEngine = MapReassembler<Uslp, M, S>;

const fn vc(n: u8) -> VcId<6> {
    VcId::<6>::new(n).unwrap()
}

const fn tc_map(id: u8) -> aldis_tc::TcMapId {
    aldis_tc::TcMapId::new(id).unwrap()
}

const fn um(id: u8) -> MapId<4> {
    MapId::<4>::new(id).unwrap()
}

/// TC VC 1: MAP 1 SDUs, MAP 2 packets, MAP 3 SDUs of at most 6 octets.
fn tc_engine() -> TcEngine {
    let maps = [
        ReassemblerMap::<Tc>::sdu(vc(1), tc_map(1), S).unwrap(),
        ReassemblerMap::<Tc>::packets(vc(1), tc_map(2), S).unwrap(),
        ReassemblerMap::<Tc>::sdu(vc(1), tc_map(3), 6).unwrap(),
    ];
    let cfg = MapReassemblerConfig::new(TcReassemblerParams::with_segment_header(), &maps);
    TcEngine::new(cfg.unwrap()).unwrap()
}

/// USLP: VC 1 has MAP 0 SDUs, MAP 1 packets, MAP 2 a stream; VC 2 has MAP 0 SDUs.
fn uslp_engine() -> UslpEngine {
    let maps = [
        ReassemblerMap::<Uslp>::sdu(vc(1), um(0), S).unwrap(),
        ReassemblerMap::<Uslp>::packets(vc(1), um(1), S).unwrap(),
        ReassemblerMap::<Uslp>::octet_stream(vc(1), um(2)),
        ReassemblerMap::<Uslp>::sdu(vc(2), um(0), 6).unwrap(),
    ];
    UslpEngine::new(MapReassemblerConfig::new((), &maps).unwrap()).unwrap()
}

/// A variable-length TFDF: rule bits, UPID 5, the TFDZ.
fn variable(rule: u8, zone: &[u8]) -> Vec<u8> {
    let mut f = vec![rule << 5 | 5];
    f.extend_from_slice(zone);
    f
}

/// A fixed-length TFDF of rule `001` or `010` with its pointer.
fn fixed(rule: u8, pointer: u16, zone: &[u8]) -> Vec<u8> {
    let mut f = vec![rule << 5 | 5];
    f.extend_from_slice(&pointer.to_be_bytes());
    f.extend_from_slice(zone);
    f
}

/// What a step returned: the events and deliveries, or the error.
#[derive(Debug, PartialEq, Eq)]
enum Got {
    Event(ReassemblyEvent, u8, u8),
    Sdu(u8, u8, Vec<u8>),
    Packets(u8, u8, Vec<Vec<u8>>),
    Stream(u8, u8, Vec<u8>, bool),
    Discarded(usize),
}

fn convert(a: &aldis_link::reassembler::ReassemblerActions, area: &[u8]) -> Vec<Got> {
    a.iter()
        .map(|o| match *o {
            ReassemblerOutput::Event { event, vc, map } => Got::Event(event, vc, map),
            ReassemblerOutput::Sdu { vc, map, sdu } => {
                Got::Sdu(vc, map, sdu.slice(area).unwrap().to_vec())
            }
            ReassemblerOutput::Packets {
                vc,
                map,
                run,
                count,
            } => {
                let p: Vec<Vec<u8>> = PacketRun::new(run, area)
                    .unwrap()
                    .map(<[u8]>::to_vec)
                    .collect();
                assert_eq!(p.len(), count);
                Got::Packets(vc, map, p)
            }
            ReassemblerOutput::StreamData {
                vc,
                map,
                data,
                after_gap,
            } => Got::Stream(vc, map, data.slice(area).unwrap().to_vec(), after_gap),
            ReassemblerOutput::Discarded { octets } => Got::Discarded(octets),
        })
        .collect()
}

type Failure = (ErrorKind, Option<aldis_core::FieldId>);

fn tc(e: &mut TcEngine, data: &[u8]) -> Result<Vec<Got>, Failure> {
    let mut area = [0xEE_u8; 64];
    let input = ReassemblerInput::Fdu(FduIn {
        vc: vc(1),
        meta: (),
        data,
    });
    let r = e.step(input, &mut area);
    r.map(|a| convert(&a, &area))
        .map_err(|e| (e.kind(), e.field()))
}

fn uslp(e: &mut UslpEngine, v: u8, map: u8, gap: bool, data: &[u8]) -> Result<Vec<Got>, Failure> {
    let mut area = [0xEE_u8; 64];
    let input = ReassemblerInput::Fdu(FduIn {
        vc: vc(v),
        meta: UslpFdu { map: um(map), gap },
        data,
    });
    let r = e.step(input, &mut area);
    r.map(|a| convert(&a, &area))
        .map_err(|e| (e.kind(), e.field()))
}

fn spp(total: usize) -> Vec<u8> {
    let mut p = vec![0x00, 0x01, 0xC0, 0x00];
    p.extend_from_slice(&u16::try_from(total - 7).unwrap().to_be_bytes());
    p.resize(total, 0x55);
    p
}

#[test]
fn configurations_are_checked() {
    // req: LNK-073, LNK-078, API-040
    let tcp = TcReassemblerParams::with_segment_header();
    let e = ReassemblerMap::<Tc>::sdu(vc(1), tc_map(1), 0).unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidConfig, Some(field::MAX_SDU_LEN))
    );
    assert!(ReassemblerMap::<Tc>::packets(vc(1), tc_map(1), 0).is_err());
    let first = ReassemblerMap::<Tc>::sdu(vc(1), tc_map(1), 8).unwrap();
    let cfg = |maps: &[ReassemblerMap<Tc>]| {
        MapReassemblerConfig::<Tc, 2>::new(tcp, maps)
            .err()
            .map(|e| (e.kind(), e.field()))
    };
    let map_id = Some((ErrorKind::InvalidConfig, Some(field::MAP_ID)));
    assert_eq!(cfg(&[]), map_id);
    assert_eq!(cfg(&[first, first]), map_id);
    // The same MAP Identifier on another VC is another MAP.
    let other_vc = ReassemblerMap::<Tc>::sdu(vc(2), tc_map(1), 8).unwrap();
    assert_eq!(cfg(&[first, other_vc]), None);
    let third = ReassemblerMap::<Tc>::sdu(vc(1), tc_map(2), 8).unwrap();
    assert_eq!(
        cfg(&[first, other_vc, third]),
        Some((ErrorKind::CapacityExceeded, Some(field::MAP_ID)))
    );
    // The maximum SDU length is checked against the capacity of the engine (LNK-073).
    let big = ReassemblerMap::<Tc>::sdu(vc(1), tc_map(1), S + 1).unwrap();
    let config = MapReassemblerConfig::<Tc, 1>::new(tcp, &[big]).unwrap();
    let e = MapReassembler::<Tc, 1, S>::new(config).unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::CapacityExceeded, Some(field::MAX_SDU_LEN))
    );
    // The accessors.
    assert_eq!(big.max_sdu_len(), Some(S + 1));
    assert_eq!(
        ReassemblerMap::<Uslp>::octet_stream(vc(1), um(0)).max_sdu_len(),
        None
    );
    assert_eq!((first.vc(), first.id()), (vc(1), tc_map(1)));
    assert_eq!(first.service(), aldis_link::reassembler::Service::Sdu);
    let engine = tc_engine();
    assert_eq!(engine.config().maps().count(), 3);
    assert_eq!(*engine.config().channel(), tcp);
    // Reconfigure checks the same and changes nothing.
    let mut engine = tc_engine();
    tc(&mut engine, &[0x41, 1, 2, 3]).unwrap();
    let mut area = [0_u8; 8];
    let r = engine.step(ReassemblerInput::Reconfigure(config_one(S + 1)), &mut area);
    assert_eq!(r.err().map(|e| e.kind()), Some(ErrorKind::CapacityExceeded));
    assert_eq!(engine.status().assembly.current(), 3);
}

fn config_one(max: usize) -> MapReassemblerConfig<Tc, M> {
    let map = ReassemblerMap::<Tc>::sdu(vc(1), tc_map(1), max).unwrap();
    MapReassemblerConfig::new(TcReassemblerParams::with_segment_header(), &[map]).unwrap()
}

#[test]
fn a_gap_discards_the_sdus_in_progress_of_every_map_of_the_vc() {
    // req: LNK-070, LNK-051; OI-78 (two SDUs in progress on one USLP VC)
    // 732.1 §4.3.3.1 note: one instance of the reception function for each MAP channel, so two
    // SDUs of one VC are in progress at once; §4.1.4.2.2.1.6: the portions of an SDU are in frames
    // of contiguous VCF Counts, which a gap breaks for whichever MAP lost a frame.
    let mut e = uslp_engine();
    assert_eq!(
        uslp(&mut e, 1, 0, false, &variable(0b100, &[1, 2, 3])),
        Ok(vec![])
    );
    assert_eq!(
        uslp(&mut e, 1, 1, false, &variable(0b100, &spp(9)[..5])),
        Ok(vec![])
    );
    assert_eq!(
        uslp(&mut e, 2, 0, false, &variable(0b100, &[9, 9])),
        Ok(vec![])
    );
    assert_eq!(e.status().assembly.current(), 3 + 5 + 2);
    // A gap flagged on a field of MAP 2 (a stream): the SDUs of VC 1 are discarded with one event
    // that names the first MAP that had one; VC 2 is not touched.
    let got = uslp(&mut e, 1, 2, true, &variable(0b011, &[4, 4])).unwrap();
    assert_eq!(
        got,
        [
            Got::Event(GapDiscard, 1, 0),
            Got::Stream(1, 2, vec![4, 4], true)
        ]
    );
    assert_eq!(e.status().assembly.current(), 2);
    assert_eq!(e.status().gap_discards.get(), 1);
    assert_eq!(e.status().discarded_octets.get(), 8);
    // The continuations that follow are sequence violations.
    assert_eq!(
        uslp(&mut e, 1, 0, false, &variable(0b110, &[3])),
        Ok(vec![Got::Event(SequenceViolation, 1, 0)])
    );
    assert_eq!(
        uslp(&mut e, 2, 0, false, &variable(0b110, &[3])),
        Ok(vec![Got::Sdu(2, 0, vec![9, 9, 3])])
    );
    // The event names the MAP of the field when an SDU of it was in progress.
    uslp(&mut e, 1, 0, false, &variable(0b100, &[1]));
    uslp(&mut e, 1, 1, false, &variable(0b100, &spp(9)[..5]));
    let got = uslp(&mut e, 1, 1, true, &variable(0b101, &[1])).unwrap();
    assert_eq!(
        got,
        [
            Got::Event(GapDiscard, 1, 1),
            Got::Event(SequenceViolation, 1, 1)
        ]
    );
    // A gap with nothing in progress raises nothing.
    assert_eq!(
        uslp(&mut e, 1, 2, true, &variable(0b011, &[5])),
        Ok(vec![Got::Stream(1, 2, vec![5], true)])
    );
}

#[test]
fn a_gap_before_an_unknown_map_still_discards() {
    // req: LNK-070, OI-78
    let mut e = uslp_engine();
    uslp(&mut e, 1, 0, false, &variable(0b100, &[1, 2]));
    let got = uslp(&mut e, 1, 9, true, &variable(0b111, &[1])).unwrap();
    assert_eq!(
        got,
        [Got::Event(GapDiscard, 1, 0), Got::Event(UnknownMap, 1, 9)]
    );
    // A VC that has no MAP at all: the frame is for no configured GMAP ID.
    assert_eq!(
        uslp(&mut e, 5, 0, true, &variable(0b111, &[1])),
        Ok(vec![Got::Event(UnknownMap, 5, 0)])
    );
    assert_eq!(e.status().unknown_maps.get(), 2);
}

#[test]
fn an_octet_stream_has_no_sdu_to_discard() {
    // req: LNK-072, LNK-070; OI-78 (a gap on an octet stream)
    let mut e = uslp_engine();
    assert_eq!(
        uslp(&mut e, 1, 2, false, &variable(0b011, &[1])),
        Ok(vec![Got::Stream(1, 2, vec![1], false)])
    );
    assert_eq!(
        uslp(&mut e, 1, 2, true, &variable(0b011, &[2])),
        Ok(vec![Got::Stream(1, 2, vec![2], true)])
    );
    assert_eq!(e.status().gap_discards.get(), 0);
    assert_eq!(e.status().delivered.get(), 2);
}

#[test]
fn a_blocked_fdu_whose_packets_do_not_fit_is_refused_and_changes_nothing() {
    // req: LNK-071, API-058; OI-78 (a blocked FDU whose packets do not fit it)
    let mut e = tc_engine();
    // A packet in progress on MAP 2 survives the refusals.
    let long = spp(14);
    tc(&mut e, &[&[0x42][..], &long[..8]].concat()).unwrap();
    let mismatch = Err((ErrorKind::LengthMismatch, Some(field::PACKET_LEN)));
    // The second packet runs past the end of the FDU.
    let blocked = [&[0xC2][..], &spp(8), &spp(8)[..5]].concat();
    assert_eq!(tc(&mut e, &blocked), mismatch);
    // A header that is not complete.
    assert_eq!(tc(&mut e, &[0xC2, 0x00, 0x01]), mismatch);
    // A PVN that cannot be delimited.
    assert_eq!(
        tc(&mut e, &[0xC2, 0x20, 0, 0, 0, 0, 0, 0]),
        Err((ErrorKind::InvalidField, Some(field::PACKET_VERSION)))
    );
    // An Encapsulation length field shorter than its header.
    assert_eq!(tc(&mut e, &[0xC2, 0xFD, 0x01, 0x01]), mismatch);
    assert_eq!(e.status().refused.get(), 4);
    assert_eq!(e.status().fdus_in.get(), 1);
    assert_eq!(e.status().assembly.current(), 8);
    // The packet in progress completes.
    let got = tc(&mut e, &[&[0x82][..], &long[8..]].concat()).unwrap();
    assert_eq!(got, [Got::Packets(1, 2, vec![long])]);
    // A good block of Space and Encapsulation Packets is one run.
    let block = [&[0xC2][..], &spp(8), &[0xFD, 0x04, 1, 2]].concat();
    assert_eq!(
        tc(&mut e, &block),
        Ok(vec![Got::Packets(
            1,
            2,
            vec![spp(8), vec![0xFD, 0x04, 1, 2]]
        )])
    );
}

#[test]
fn a_reassembled_packet_must_agree_with_its_length_field() {
    // req: LNK-070, API-058
    let mut e = tc_engine();
    tc(&mut e, &[&[0x42][..], &spp(14)[..8]].concat()).unwrap();
    // The last segment ends the packet one octet early: refused, the SDU is still in progress.
    assert_eq!(
        tc(&mut e, &[&[0x82][..], &spp(14)[8..13]].concat()),
        Err((ErrorKind::LengthMismatch, Some(field::PACKET_LEN)))
    );
    assert_eq!(e.status().assembly.current(), 8);
    let got = tc(&mut e, &[&[0x82][..], &spp(14)[8..]].concat()).unwrap();
    assert_eq!(got, [Got::Packets(1, 2, vec![spp(14)])]);
    // A first segment that is no packet is found when the packet ends.
    tc(&mut e, &[0x42, 0x20, 1, 2, 3, 4, 5]).unwrap();
    assert_eq!(
        tc(&mut e, &[0x82, 6, 7]),
        Err((ErrorKind::InvalidField, Some(field::PACKET_VERSION)))
    );
}

#[test]
fn a_last_valid_octet_pointer_beyond_the_zone_is_malformed() {
    // req: LNK-072, API-058; OI-78 (a Last Valid Octet Pointer beyond the zone)
    let mut e = uslp_engine();
    uslp(&mut e, 1, 0, false, &fixed(0b001, 0xFFFF, &[1, 2, 3, 4]));
    let beyond = Err((ErrorKind::Malformed, Some(field::FHP_LVO_POINTER)));
    assert_eq!(
        uslp(&mut e, 1, 0, false, &fixed(0b010, 4, &[5, 6, 7, 8])),
        beyond
    );
    assert_eq!(
        uslp(&mut e, 1, 0, false, &fixed(0b001, 9, &[5, 6, 7, 8])),
        beyond
    );
    // The pointer at the last octet is fine, and the SDU in progress was kept.
    assert_eq!(
        uslp(&mut e, 1, 0, false, &fixed(0b010, 3, &[5, 6, 7, 8])),
        Ok(vec![Got::Sdu(1, 0, vec![1, 2, 3, 4, 5, 6, 7, 8])])
    );
    // A fixed-length TFDF needs its pointer and an octet of TFDZ.
    let short = Err((ErrorKind::InvalidField, Some(field::FRAME_DATA_FIELD)));
    assert_eq!(uslp(&mut e, 1, 0, false, &[0x25, 0, 3]), short);
    assert_eq!(uslp(&mut e, 1, 0, false, &[0x25, 0]), short);
    assert_eq!(uslp(&mut e, 1, 0, false, &[]), short);
    assert_eq!(uslp(&mut e, 1, 0, false, &[0xE5]), short);
}

#[test]
fn a_rule_of_another_service_is_refused() {
    // req: LNK-070, LNK-072; OI-78 (a rule on a MAP of another service)
    // 732.1 §4.1.4.2.2.2: rules 001 and 010 carry a MAPA_SDU or VCA_SDU, 011 an octet stream,
    // 100..111 an SDU or packets, 000 packets of a fixed-length TFDZ (the PacketExtractor's).
    let mut e = uslp_engine();
    let rule = Err((ErrorKind::InvalidField, Some(field::TFDZ_CONSTRUCTION_RULE)));
    // MAP 0 (SDUs): no octet stream, no rule 000.
    assert_eq!(uslp(&mut e, 1, 0, false, &variable(0b011, &[1])), rule);
    assert_eq!(uslp(&mut e, 1, 0, false, &[0x05, 0, 0, 1]), rule);
    // MAP 1 (packets): no fixed-length SDUs, no octet stream.
    assert_eq!(uslp(&mut e, 1, 1, false, &fixed(0b001, 0xFFFF, &[1])), rule);
    assert_eq!(uslp(&mut e, 1, 1, false, &fixed(0b010, 0, &[1])), rule);
    assert_eq!(uslp(&mut e, 1, 1, false, &variable(0b011, &[1])), rule);
    // MAP 2 (stream): no segments, no whole SDUs, no fixed-length SDUs.
    for r in [0b100, 0b101, 0b110, 0b111] {
        assert_eq!(uslp(&mut e, 1, 2, false, &variable(r, &[1])), rule);
    }
    assert_eq!(uslp(&mut e, 1, 2, false, &fixed(0b001, 0, &[1])), rule);
    assert_eq!(e.status().refused.get(), 10);
    assert_eq!(e.status().fdus_in.get(), 0);
    // Nothing was delivered, counted, or changed; the right rules work.
    assert_eq!(
        uslp(&mut e, 1, 0, false, &fixed(0b001, 0, &[7, 8])),
        Ok(vec![Got::Sdu(1, 0, vec![7])])
    );
    assert_eq!(
        uslp(&mut e, 1, 2, false, &variable(0b011, &[1])),
        Ok(vec![Got::Stream(1, 2, vec![1], false)])
    );
}

#[test]
fn an_idle_tfdf_carries_nothing() {
    // req: LNK-064, LNK-070 (732.1 §4.3.2.2.5 note 1)
    let mut e = uslp_engine();
    uslp(&mut e, 1, 0, false, &variable(0b100, &[1, 2]));
    // UPID 31, Only Idle Data, rule 001 as an OID frame has it.
    assert_eq!(
        uslp(&mut e, 1, 0, false, &[0x3F, 0, 3, 0, 0, 0, 0]),
        Ok(vec![])
    );
    assert_eq!(e.status().assembly.current(), 2);
}

#[test]
fn an_sdu_longer_than_its_maximum_is_discarded_with_an_event() {
    // req: LNK-073, LNK-070; OI-78 (the maximum SDU size)
    let mut e = tc_engine();
    let long = Ok(vec![Got::Event(SduTooLong, 1, 3)]);
    // MAP 3 holds at most 6 octets. A whole SDU of 7, a first segment of 7: discarded.
    assert_eq!(tc(&mut e, &[0xC3, 1, 2, 3, 4, 5, 6, 7]), long);
    assert_eq!(tc(&mut e, &[0x43, 1, 2, 3, 4, 5, 6, 7]), long);
    // 4 + 3 octets by a continuing segment: the partial SDU is discarded too.
    tc(&mut e, &[0x43, 1, 2, 3, 4]);
    assert_eq!(tc(&mut e, &[0x03, 5, 6, 7]), long);
    assert_eq!(e.status().assembly.current(), 0);
    assert_eq!(
        tc(&mut e, &[0x83, 5]),
        Ok(vec![Got::Event(SequenceViolation, 1, 3)])
    );
    // By the last segment.
    tc(&mut e, &[0x43, 1, 2, 3, 4]);
    assert_eq!(tc(&mut e, &[0x83, 5, 6, 7]), long);
    // Exactly 6 is fine, in one piece and in three.
    assert_eq!(
        tc(&mut e, &[0xC3, 1, 2, 3, 4, 5, 6]),
        Ok(vec![Got::Sdu(1, 3, vec![1, 2, 3, 4, 5, 6])])
    );
    tc(&mut e, &[0x43, 1, 2]);
    tc(&mut e, &[0x03, 3, 4]);
    assert_eq!(
        tc(&mut e, &[0x83, 5, 6]),
        Ok(vec![Got::Sdu(1, 3, vec![1, 2, 3, 4, 5, 6])])
    );
    assert_eq!(e.status().sdus_too_long.get(), 4);
    assert_eq!(e.status().discarded_octets.get(), 4 + 4);
    // A whole SDU of too many octets while another is in progress: both events, in order.
    tc(&mut e, &[0x43, 1]);
    assert_eq!(
        tc(&mut e, &[0xC3, 1, 2, 3, 4, 5, 6, 7]),
        Ok(vec![
            Got::Event(SequenceViolation, 1, 3),
            Got::Event(SduTooLong, 1, 3)
        ])
    );
}

#[test]
fn no_input_exceeds_two_actions() {
    // req: LNK-074, API-057
    assert_eq!(MAP_REASSEMBLER_MAX_ACTIONS, 2);
    let mut most = 0;
    let mut note = |r: Result<Vec<Got>, Failure>| most = most.max(r.unwrap().len());
    // A gap, then a continuation with nothing to continue: two events.
    let mut e = uslp_engine();
    uslp(&mut e, 1, 0, false, &variable(0b100, &[1]));
    let two = uslp(&mut e, 1, 0, true, &variable(0b101, &[2]));
    assert_eq!(
        two,
        Ok(vec![
            Got::Event(GapDiscard, 1, 0),
            Got::Event(SequenceViolation, 1, 0)
        ])
    );
    note(two);
    // A violation, then the delivery of the whole SDU.
    uslp(&mut e, 1, 0, false, &variable(0b100, &[1]));
    let two = uslp(&mut e, 1, 0, false, &variable(0b111, &[2]));
    assert_eq!(
        two,
        Ok(vec![
            Got::Event(SequenceViolation, 1, 0),
            Got::Sdu(1, 0, vec![2])
        ])
    );
    note(two);
    // A gap, then the delivery: the gap leaves no SDU to violate.
    uslp(&mut e, 1, 0, false, &variable(0b100, &[1]));
    let two = uslp(&mut e, 1, 0, true, &variable(0b111, &[3]));
    assert_eq!(
        two,
        Ok(vec![Got::Event(GapDiscard, 1, 0), Got::Sdu(1, 0, vec![3])])
    );
    note(two);
    // Packets in a block are one action however many there are.
    let block = [spp(8), spp(8)].concat();
    let one = uslp(&mut e, 1, 1, false, &variable(0b111, &block));
    assert_eq!(one.as_ref().map(Vec::len), Ok(1));
    note(one);
    assert_eq!(most, MAP_REASSEMBLER_MAX_ACTIONS);
}

#[test]
fn an_area_that_is_too_short_fails_the_input_and_changes_nothing() {
    // req: LNK-075, API-058
    let mut e = tc_engine();
    tc(&mut e, &[0x41, 1, 2, 3]);
    // MAP 1 holds 16: an FDU of 4 octets needs 16 - 1 + 4 = 19.
    let mut short = [0_u8; 18];
    let data = [0x81, 4, 5, 6, 7];
    let input = ReassemblerInput::Fdu(FduIn {
        vc: vc(1),
        meta: (),
        data: &data,
    });
    let r = e.step(input, &mut short);
    assert_eq!(r.err().map(|e| e.kind()), Some(ErrorKind::BufferTooSmall));
    assert_eq!(e.status().assembly.current(), 3);
    let mut area = [0_u8; 19];
    let a = e.step(input, &mut area).unwrap();
    assert_eq!(
        convert(&a, &area),
        [Got::Sdu(1, 1, vec![1, 2, 3, 4, 5, 6, 7])]
    );
    // An event needs no area: an unknown MAP is reported with none.
    let mut none = [0_u8; 0];
    let unknown = ReassemblerInput::Fdu(FduIn {
        vc: vc(1),
        meta: (),
        data: &[0xC9, 1],
    });
    assert_eq!(e.step(unknown, &mut none).unwrap().len(), 1);
}

#[test]
fn a_tc_fdu_needs_user_data_and_a_vc_without_a_segment_header_is_one_map() {
    // req: LNK-070, LNK-071 (232.0 §4.1.3.2.1, §4.4.4.3)
    let mut e = tc_engine();
    let none = Err((ErrorKind::InvalidField, Some(field::FRAME_DATA_FIELD)));
    assert_eq!(tc(&mut e, &[]), none);
    assert_eq!(tc(&mut e, &[0xC1]), none);
    let packets = ReassemblerMap::<Tc>::packets(vc(1), tc_map(0), S).unwrap();
    let plain = TcReassemblerParams::without_segment_header();
    let cfg = MapReassemblerConfig::<Tc, M>::new(plain, &[packets]).unwrap();
    let mut e = TcEngine::new(cfg).unwrap();
    assert_eq!(tc(&mut e, &[]), none);
    // The whole FDU is packets, whatever its first octet looks like.
    let block = [spp(8), vec![0xFD, 0x04, 1, 2]].concat();
    assert_eq!(
        tc(&mut e, &block),
        Ok(vec![Got::Packets(
            1,
            0,
            vec![spp(8), vec![0xFD, 0x04, 1, 2]]
        )])
    );
    // A VC that is not configured is a MAP that is not configured.
    let mut area = [0_u8; 64];
    let other = ReassemblerInput::Fdu(FduIn {
        vc: vc(2),
        meta: (),
        data: &block,
    });
    let a = e.step(other, &mut area).unwrap();
    assert_eq!(convert(&a, &area), [Got::Event(UnknownMap, 2, 0)]);
}

#[test]
fn reconfigure_discards_the_sdus_in_progress_and_takes_the_new_maps() {
    // req: LNK-003, LNK-002
    let mut e = uslp_engine();
    uslp(&mut e, 1, 0, false, &variable(0b100, &[1, 2, 3]));
    uslp(&mut e, 2, 0, false, &variable(0b100, &[4, 5]));
    let mut area = [0_u8; 8];
    let map = ReassemblerMap::<Uslp>::sdu(vc(3), um(7), S).unwrap();
    let cfg = MapReassemblerConfig::<Uslp, M>::new((), &[map]).unwrap();
    let a = e
        .step(ReassemblerInput::Reconfigure(cfg), &mut area)
        .unwrap();
    assert_eq!(convert(&a, &area), [Got::Discarded(5)]);
    assert_eq!(e.status().discarded_octets.get(), 5);
    assert_eq!(e.status().assembly.current(), 0);
    assert_eq!(e.status().assembly.high_water(), 5);
    assert_eq!(
        uslp(&mut e, 1, 0, false, &variable(0b111, &[1])),
        Ok(vec![Got::Event(UnknownMap, 1, 0)])
    );
    assert_eq!(
        uslp(&mut e, 3, 7, false, &variable(0b111, &[1])),
        Ok(vec![Got::Sdu(3, 7, vec![1])])
    );
    // Nothing in progress: no action.
    let a = e
        .step(ReassemblerInput::Reconfigure(cfg), &mut area)
        .unwrap();
    assert!(a.is_empty());
}

#[test]
fn the_status_counts_what_happened() {
    // req: LNK-002, API-056
    let mut e = tc_engine();
    tc(&mut e, &[0x41, 1, 2]);
    tc(&mut e, &[0x01, 3]);
    tc(&mut e, &[0x81, 4]);
    tc(&mut e, &[0xC9, 1]);
    tc(&mut e, &[0x81, 4]);
    let s = e.status();
    assert_eq!((s.fdus_in.get(), s.delivered.get()), (5, 1));
    assert_eq!((s.unknown_maps.get(), s.sequence_violations.get()), (1, 1));
    assert_eq!(s.assembly.high_water(), 3);
    assert_eq!(
        e.config().maps().next().map(ReassemblerMap::max_sdu_len),
        Some(Some(S))
    );
    for (event, n) in [
        (GapDiscard, 0),
        (SduTooLong, 0),
        (UnknownMap, 1),
        (SequenceViolation, 1),
    ] {
        let counted = match event {
            GapDiscard => s.gap_discards.get(),
            SduTooLong => s.sdus_too_long.get(),
            UnknownMap => s.unknown_maps.get(),
            SequenceViolation => s.sequence_violations.get(),
        };
        assert_eq!(counted, n, "{event:?}");
    }
}
