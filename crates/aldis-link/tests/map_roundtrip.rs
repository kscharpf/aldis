//! Round trip of the `MapSegmenter` and the `MapReassembler` (VER-023, LNK-040…LNK-042, LNK-070…
//! LNK-072) and the no-`InternalError` property of the three WP-6 engines (LNK-074, API-057): the
//! packets, SDUs, and octets a segmenter turns into FDUs and TFDFs come out of a reassembler as
//! they went in, in order, with no event, whatever the FDU length, the blocking, the service
//! (sequence-controlled or expedited) of each SDU, and the release of a stream.
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    unused_must_use,
    reason = "tests fail loudly by design"
)]

use aldis_core::ErrorKind;
use aldis_core::ids::{MapId, VcId};
use aldis_core::time::{Duration, Instant};
use aldis_link::cltu::{CltuInput, CltuPacker, CltuPackerConfig, FrameIn, Repetitions};
use aldis_link::common::Qos;
use aldis_link::extractor::PacketRun;
use aldis_link::protocol::{Tc, Uslp};
use aldis_link::reassembler::{
    FduIn, MapReassembler, MapReassemblerConfig, ReassemblerInput, ReassemblerMap,
    ReassemblerOutput, TcReassemblerParams, UslpFdu,
};
use aldis_link::segmenter::{
    Framing, MapEntry, MapSegmenter, MapSegmenterConfig, MapService, SegmenterInput,
    SegmenterOutput, TcSegmenterParams, UslpSegmenterParams,
};
use aldis_testkit::strategies;
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::proptest::test_runner::{Config, RngSeed};
use aldis_uslp::{Upid, VcfCountLen};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/map_roundtrip.txt"
);

const T0: Instant = Instant::from_micros(0);
/// The longest SDU, packet, and queue of the engines under test.
const MAX_SDU: usize = 60;
const Q: usize = 1024;
const AREA: usize = 256;

const fn vc() -> VcId<6> {
    VcId::<6>::new(1).unwrap()
}

/// A Space Packet of `total` octets (7 to 60), or an Encapsulation Packet when `total` is below 7
/// (3 to 6 octets).
fn packet(total: usize, fill: u8) -> Vec<u8> {
    if total >= 7 {
        let mut p = vec![0x00, 0x01, 0xC0, 0x00];
        p.extend_from_slice(&u16::try_from(total - 7).unwrap().to_be_bytes());
        p.resize(total, fill);
        p
    } else {
        let mut p = vec![0xFD, u8::try_from(total).unwrap()];
        p.resize(total, fill);
        p
    }
}

/// What was put in for each MAP, in order: SDUs, or packets.
#[derive(Debug, Clone)]
enum Item {
    Sdu(u8, Vec<u8>),
    Packet(u8, Vec<u8>),
}

fn sdu_data() -> impl Strategy<Value = Vec<u8>> {
    strategies::octets(1..=MAX_SDU)
}

fn tc_items() -> impl Strategy<Value = Vec<Item>> {
    prop::collection::vec(
        prop_oneof![
            (prop_oneof![Just(1_u8), Just(3_u8)], sdu_data()).prop_map(|(m, d)| Item::Sdu(m, d)),
            (3_usize..=MAX_SDU, any::<u8>()).prop_map(|(n, f)| Item::Packet(2, packet(n, f))),
        ],
        1..14,
    )
}

fn uslp_items() -> impl Strategy<Value = Vec<(Option<Item>, Vec<u8>)>> {
    prop::collection::vec(
        prop_oneof![
            sdu_data().prop_map(|d| (Some(Item::Sdu(2, d)), Vec::new())),
            (3_usize..=MAX_SDU, any::<u8>())
                .prop_map(|(n, f)| (Some(Item::Packet(1, packet(n, f))), Vec::new())),
            strategies::octets(1..=30).prop_map(|d| (None, d)),
        ],
        1..14,
    )
}

const fn qos_of(bit: bool) -> Qos {
    if bit {
        Qos::Expedited
    } else {
        Qos::SequenceControlled
    }
}

/// What a reassembler delivered: per MAP, the SDUs, or the packets.
#[derive(Debug, Default, PartialEq, Eq)]
struct Delivered {
    sdus: Vec<(u8, Vec<u8>)>,
    packets: Vec<(u8, Vec<u8>)>,
    stream: Vec<u8>,
    events: usize,
}

fn collect(actions: &aldis_link::reassembler::ReassemblerActions, area: &[u8], d: &mut Delivered) {
    for o in actions.iter() {
        match *o {
            ReassemblerOutput::Sdu { map, sdu, .. } => {
                d.sdus.push((map, sdu.slice(area).unwrap().to_vec()));
            }
            ReassemblerOutput::Packets { map, run, .. } => {
                for p in PacketRun::new(run, area).unwrap() {
                    d.packets.push((map, p.to_vec()));
                }
            }
            ReassemblerOutput::StreamData { data, .. } => {
                d.stream.extend_from_slice(data.slice(area).unwrap());
            }
            ReassemblerOutput::Event { .. } | ReassemblerOutput::Discarded { .. } => d.events += 1,
        }
    }
}

/// The expected deliveries of the items, in the order of submission.
fn expected(items: &[Item]) -> Delivered {
    let mut d = Delivered::default();
    for i in items {
        match i {
            Item::Sdu(m, data) => d.sdus.push((*m, data.clone())),
            Item::Packet(m, p) => d.packets.push((*m, p.clone())),
        }
    }
    d
}

fn tc_trip(fdu_len: usize, items: &[Item], qos_bits: &[bool]) -> Result<Delivered, String> {
    let map = |id: u8| MapEntry::<Tc>::new(MapId::<6>::new(id).unwrap(), MapService::Any, ());
    let maps = [
        map(1).with_segmentation(MAX_SDU).unwrap(),
        map(2).with_segmentation(MAX_SDU).unwrap().with_blocking(),
        map(3).with_segmentation(MAX_SDU).unwrap(),
    ];
    let config = MapSegmenterConfig::<Tc, 4>::new(
        vc(),
        TcSegmenterParams::with_segment_header(false),
        fdu_len,
        &maps,
    )
    .unwrap();
    let mut seg = MapSegmenter::<Tc, 4, Q>::new(config).unwrap();
    let mut area = vec![0_u8; AREA];
    for i in items {
        let input = match i {
            Item::Sdu(m, d) => SegmenterInput::SubmitSdu {
                map: MapId::<6>::new(*m).unwrap(),
                sdu: d,
            },
            Item::Packet(m, p) => SegmenterInput::SubmitPacket {
                map: MapId::<6>::new(*m).unwrap(),
                packet: p,
            },
        };
        seg.step(input, T0, &mut area)
            .map_err(|e| format!("submit {e}"))?;
    }
    let rmaps = [
        ReassemblerMap::<Tc>::sdu(vc(), MapId::<6>::new(1).unwrap(), MAX_SDU).unwrap(),
        ReassemblerMap::<Tc>::packets(vc(), MapId::<6>::new(2).unwrap(), MAX_SDU).unwrap(),
        ReassemblerMap::<Tc>::sdu(vc(), MapId::<6>::new(3).unwrap(), MAX_SDU).unwrap(),
    ];
    let rconfig =
        MapReassemblerConfig::<Tc, 4>::new(TcReassemblerParams::with_segment_header(), &rmaps)
            .unwrap();
    let mut reas = MapReassembler::<Tc, 4, MAX_SDU>::new(rconfig).unwrap();
    let mut delivered = Delivered::default();
    let mut qos = qos_of(false);
    let mut bits = qos_bits.iter().copied().cycle();
    let mut fresh = true;
    let mut count = 0;
    // bound: every FDU takes octets off the queue, and the queue ends: at most one FDU for each octet.
    loop {
        if fresh {
            qos = qos_of(bits.next().unwrap());
        }
        let a = seg
            .step(SegmenterInput::NextFdu { handle: 1, qos }, T0, &mut area)
            .map_err(|e| format!("next {e}"))?;
        let Some(SegmenterOutput::Fdu(f)) = a.get(0).copied() else {
            break;
        };
        fresh = matches!(f.framing, Framing::Whole | Framing::Last);
        let data = f.fdu.slice(&area).unwrap().to_vec();
        let mut out = vec![0_u8; AREA];
        let input = ReassemblerInput::Fdu(FduIn {
            vc: vc(),
            meta: (),
            data: &data,
        });
        let r = reas.step(input, &mut out).map_err(|e| format!("fdu {e}"))?;
        collect(&r, &out, &mut delivered);
        count += 1;
        if count > 10_000 {
            return Err("no end".to_owned());
        }
    }
    Ok(delivered)
}

#[expect(
    clippy::too_many_lines,
    reason = "one straight run of the segmenter into the reassembler for the three services"
)]
fn uslp_trip(
    fdu_len: usize,
    items: &[(Option<Item>, Vec<u8>)],
    qos_bits: &[bool],
) -> Result<(Delivered, Vec<Item>), String> {
    let map = |id: u8, service, upid: u8| {
        MapEntry::<Uslp>::new(
            MapId::<4>::new(id).unwrap(),
            service,
            Upid::from_bits(upid).unwrap(),
        )
    };
    let two = VcfCountLen::new(2).unwrap();
    let maps = [
        map(1, MapService::Packets, 0)
            .with_segmentation(MAX_SDU)
            .unwrap()
            .with_blocking(),
        map(2, MapService::Sdu, 5)
            .with_segmentation(MAX_SDU)
            .unwrap(),
        map(
            3,
            MapService::OctetStream {
                release_deadline: Duration::from_micros(100),
            },
            4,
        ),
    ];
    let config = MapSegmenterConfig::<Uslp, 4>::new(
        vc(),
        UslpSegmenterParams::new(two, two),
        fdu_len,
        &maps,
    )
    .unwrap();
    let mut seg = MapSegmenter::<Uslp, 4, Q>::new(config).unwrap();
    let mut area = vec![0_u8; AREA];
    let mut sent = Vec::new();
    for (item, stream) in items {
        let input = match item {
            Some(Item::Sdu(m, d)) => {
                sent.push(item.clone().unwrap());
                SegmenterInput::SubmitSdu {
                    map: MapId::<4>::new(*m).unwrap(),
                    sdu: d,
                }
            }
            Some(Item::Packet(m, p)) => {
                sent.push(item.clone().unwrap());
                SegmenterInput::SubmitPacket {
                    map: MapId::<4>::new(*m).unwrap(),
                    packet: p,
                }
            }
            None => SegmenterInput::SubmitOctets {
                map: MapId::<4>::new(3).unwrap(),
                octets: stream,
            },
        };
        seg.step(input, T0, &mut area)
            .map_err(|e| format!("submit {e}"))?;
    }
    let rmaps = [
        ReassemblerMap::<Uslp>::packets(vc(), MapId::<4>::new(1).unwrap(), MAX_SDU).unwrap(),
        ReassemblerMap::<Uslp>::sdu(vc(), MapId::<4>::new(2).unwrap(), MAX_SDU).unwrap(),
        ReassemblerMap::<Uslp>::octet_stream(vc(), MapId::<4>::new(3).unwrap()),
    ];
    let rconfig = MapReassemblerConfig::<Uslp, 4>::new((), &rmaps).unwrap();
    let mut reas = MapReassembler::<Uslp, 4, MAX_SDU>::new(rconfig).unwrap();
    let mut delivered = Delivered::default();
    let mut bits = qos_bits.iter().copied().cycle();
    let mut qos = qos_of(false);
    let mut fresh = true;
    let mut now = 0_u64;
    let mut count = 0;
    // bound: every FDU takes octets off the queue, and a step with nothing to give ends the loop.
    loop {
        if fresh {
            qos = qos_of(bits.next().unwrap());
        }
        let at = Instant::from_micros(now);
        let a = seg
            .step(SegmenterInput::NextFdu { handle: 1, qos }, at, &mut area)
            .map_err(|e| format!("next {e}"))?;
        let Some(SegmenterOutput::Fdu(f)) = a.get(0).copied() else {
            // A stream TFDZ that is not full waits for its deadline.
            if let Some(due) = seg.next_deadline() {
                now = due.as_micros();
                seg.step(
                    SegmenterInput::Timeout,
                    Instant::from_micros(now),
                    &mut area,
                )
                .map_err(|e| format!("timeout {e}"))?;
                continue;
            }
            break;
        };
        fresh = matches!(f.framing, Framing::Whole | Framing::Last | Framing::Stream);
        let data = f.fdu.slice(&area).unwrap().to_vec();
        let mut out = vec![0_u8; AREA];
        let meta = UslpFdu {
            map: MapId::<4>::new(f.map.unwrap()).unwrap(),
            gap: false,
        };
        let input = ReassemblerInput::Fdu(FduIn {
            vc: vc(),
            meta,
            data: &data,
        });
        let r = reas.step(input, &mut out).map_err(|e| format!("fdu {e}"))?;
        collect(&r, &out, &mut delivered);
        count += 1;
        if count > 10_000 {
            return Err("no end".to_owned());
        }
    }
    Ok((delivered, sent))
}

fn check(d: &Delivered, want: &Delivered) -> Result<(), TestCaseError> {
    prop_assert_eq!(d.events, 0, "events");
    prop_assert_eq!(&d.sdus, &want.sdus);
    prop_assert_eq!(&d.packets, &want.packets);
    prop_assert_eq!(&d.stream, &want.stream);
    Ok(())
}

/// The configuration of the properties with a fixed seed, so that the regions they reach, and the
/// coverage waivers that count the regions they do not, are the same on every run (VER-024).
fn seeded() -> Config {
    Config {
        rng_seed: RngSeed::Fixed(0x4D41_5036),
        ..strategies::config(256, REGRESSIONS)
    }
}

proptest! {
    #![proptest_config(seeded())]

    #[test]
    fn tc_round_trip(
        // The FDU holds the Segment Header and 1 to 40 octets: SDUs and packets split in every
        // way, and the blocked packets of MAP 2 of 3 to 6 octets fit one FDU or do not.
        fdu_len in 2_usize..=41,
        items in tc_items(),
        qos_bits in prop::collection::vec(any::<bool>(), 1..8),
    ) {
        // req: VER-023, LNK-040, LNK-041, LNK-070, LNK-071
        let delivered = tc_trip(fdu_len, &items, &qos_bits).map_err(TestCaseError::fail)?;
        check(&delivered, &expected(&items))?;
    }

    #[test]
    fn uslp_round_trip(
        // The TFDF holds the TFDF Header and 1 to 40 octets.
        fdu_len in 2_usize..=41,
        items in uslp_items(),
        qos_bits in prop::collection::vec(any::<bool>(), 1..8),
    ) {
        // req: VER-023, LNK-040, LNK-041, LNK-042, LNK-070, LNK-071, LNK-072
        let (delivered, sent) = uslp_trip(fdu_len, &items, &qos_bits).map_err(TestCaseError::fail)?;
        let mut want = expected(&sent);
        want.stream = items.iter().flat_map(|(_, s)| s.clone()).collect();
        check(&delivered, &want)?;
    }

    #[test]
    fn the_segmenter_never_reports_an_internal_error(
        steps in prop::collection::vec(
            (0_u8..6, 0_u8..5, strategies::octets(0..=24), any::<bool>(), 0_u64..400),
            1..40,
        ),
    ) {
        // req: LNK-074, API-057, API-058
        let two = VcfCountLen::new(2).unwrap();
        let map = |id: u8, service, upid: u8| {
            MapEntry::<Uslp>::new(
                MapId::<4>::new(id).unwrap(),
                service,
                Upid::from_bits(upid).unwrap(),
            )
        };
        let maps = [
            map(1, MapService::Packets, 0).with_segmentation(24).unwrap().with_blocking(),
            map(2, MapService::Sdu, 5).with_segmentation(24).unwrap(),
            map(3, MapService::OctetStream { release_deadline: Duration::from_micros(50) }, 4),
        ];
        let config = MapSegmenterConfig::<Uslp, 4>::new(
            vc(),
            UslpSegmenterParams::new(two, VcfCountLen::NONE),
            8,
            &maps,
        )
        .unwrap();
        let mut seg = MapSegmenter::<Uslp, 4, 96>::new(config).unwrap();
        let mut area = [0_u8; 8];
        let mut now = 0_u64;
        for (kind, map, data, exp, dt) in &steps {
            now += dt;
            let m = MapId::<4>::new(*map % 4).unwrap();
            let input = match kind {
                0 => SegmenterInput::SubmitSdu { map: m, sdu: data },
                1 => SegmenterInput::SubmitPacket { map: m, packet: data },
                2 => SegmenterInput::SubmitOctets { map: m, octets: data },
                3 => SegmenterInput::NextFdu { handle: 1, qos: qos_of(*exp) },
                4 => SegmenterInput::Timeout,
                _ => SegmenterInput::Reconfigure(config),
            };
            if let Err(e) = seg.step(input, Instant::from_micros(now), &mut area) {
                prop_assert_ne!(e.kind(), ErrorKind::InternalError);
            }
        }
    }

    #[test]
    fn the_reassembler_never_reports_an_internal_error(
        fdus in prop::collection::vec(
            (1_u8..3, 0_u8..4, any::<bool>(), strategies::octets(0..=24), any::<bool>()),
            1..40,
        ),
    ) {
        // req: LNK-074, API-057, API-058
        let maps = [
            ReassemblerMap::<Uslp>::packets(vc(), MapId::<4>::new(1).unwrap(), 20).unwrap(),
            ReassemblerMap::<Uslp>::sdu(vc(), MapId::<4>::new(2).unwrap(), 12).unwrap(),
            ReassemblerMap::<Uslp>::octet_stream(vc(), MapId::<4>::new(3).unwrap()),
        ];
        let config = MapReassemblerConfig::<Uslp, 4>::new((), &maps).unwrap();
        let mut reas = MapReassembler::<Uslp, 4, 20>::new(config).unwrap();
        let mut area = [0_u8; 64];
        for (rule, map, gap, data, reconfigure) in &fdus {
            // A random rule bits and UPID in the first octet, then random octets.
            let mut tfdf = data.clone();
            if let Some(first) = tfdf.first_mut() {
                *first ^= *rule << 6;
            }
            let meta = UslpFdu { map: MapId::<4>::new(*map).unwrap(), gap: *gap };
            let input = if *reconfigure {
                ReassemblerInput::Reconfigure(config)
            } else {
                ReassemblerInput::Fdu(FduIn { vc: vc(), meta, data: &tfdf })
            };
            if let Err(e) = reas.step(input, &mut area) {
                prop_assert_ne!(e.kind(), ErrorKind::InternalError);
            }
        }
    }

    #[test]
    fn the_packer_never_reports_an_internal_error(
        frames in prop::collection::vec(
            (1_usize..=14, any::<bool>(), 1_u16..4, any::<bool>(), any::<bool>()),
            1..40,
        ),
    ) {
        // req: LNK-074, API-057, LNK-044
        let config = CltuPackerConfig::<Uslp>::new(3, 20).unwrap();
        let mut packer = CltuPacker::<Uslp, 20>::new(config).unwrap();
        let mut area = [0_u8; 40];
        for (len, exp, reps, truncated, flush) in &frames {
            let data = vec![0_u8; *len];
            let input = if *flush {
                CltuInput::Flush
            } else {
                CltuInput::AddFrame(FrameIn {
                    frame: &data,
                    qos: qos_of(*exp),
                    repetitions: Repetitions::new(*reps).unwrap(),
                    truncated: *truncated,
                })
            };
            if let Err(e) = packer.step(input, &mut area) {
                prop_assert_ne!(e.kind(), ErrorKind::InternalError);
            }
        }
    }
}
