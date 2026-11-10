//! The `MapSegmenter` into the `MapReassembler` (VER-022, LNK-040…LNK-042, LNK-070…LNK-072,
//! LNK-074, API-057): the packets, SDUs, and octets of a TC (Segment Headers) or a USLP VC
//! (SDUs, blocked packets, an octet stream), cut into FDUs of the length the input chooses, come
//! out of a reassembler as they went in, in order, with no event. Neither engine reports an
//! error for a scenario the harness built.
#![no_main]

// req: VER-022, VER-023, LNK-040, LNK-041, LNK-042, LNK-070, LNK-071, LNK-072, LNK-074, API-057

use aldis_core::ids::{MapId, VcId};
use aldis_core::time::{Duration, Instant};
use aldis_link::common::Qos;
use aldis_link::extractor::PacketRun;
use aldis_link::protocol::{Tc, Uslp};
use aldis_link::reassembler::{
    FduIn, MapReassembler, MapReassemblerConfig, ReassemblerInput, ReassemblerMap,
    ReassemblerOutput, TcReassemblerParams, UslpFdu,
};
use aldis_link::segmenter::{
    Framing, MapEntry, MapSegmenter, MapSegmenterConfig, SegmenterInput, SegmenterOutput,
    SegmenterService, TcSegmenterParams, UslpSegmenterParams,
};
use aldis_uslp::{Upid, VcfCountLen};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

/// The physical channel of a TC VC: the longest frame the standard allows.
fn tc_physical(fecf: bool) -> aldis_tc::PhysicalChannelConfig {
    aldis_tc::PhysicalChannelConfig::new(1024, fecf).unwrap()
}

/// The segmenter's parameters of a variable-length USLP VC with the VCF Count Lengths `sc` and
/// `exp`.
fn uslp_params(sc: VcfCountLen, exp: VcfCountLen) -> UslpSegmenterParams {
    let mut vc = aldis_uslp::VcParams::new(aldis_uslp::TransferFrameType::Variable);
    vc.vcf_count_len_sequence = sc;
    vc.vcf_count_len_expedited = exp;
    UslpSegmenterParams::new(vc).unwrap()
}

const T0: Instant = Instant::from_micros(0);
const MAX_SDU: usize = 60;
const Q: usize = 1024;
const AREA: usize = 256;

const fn vc() -> VcId<6> {
    VcId::<6>::new(1).unwrap()
}

/// A Space Packet of `total` octets (7 to 60), or an Encapsulation Packet of 3 to 6 octets.
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

/// What was put in for each MAP.
#[derive(Debug, Clone)]
enum Item {
    Sdu(u8, Vec<u8>),
    Packet(u8, Vec<u8>),
}

const fn qos_of(bit: bool) -> Qos {
    if bit {
        Qos::Expedited
    } else {
        Qos::SequenceControlled
    }
}

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

/// Up to `max` octets of the input, at least one.
fn octets(u: &mut Unstructured<'_>, max: usize) -> Result<Vec<u8>> {
    let n = u.int_in_range(1..=max)?.min(u.len());
    if n == 0 {
        return Err(libfuzzer_sys::arbitrary::Error::NotEnoughData);
    }
    Ok(u.bytes(n)?.to_vec())
}

fn qos_bits(u: &mut Unstructured<'_>) -> Result<Vec<bool>> {
    (0..u.int_in_range(1..=8)?).map(|_| u.arbitrary()).collect()
}

fn tc_trip(u: &mut Unstructured<'_>) -> Result<()> {
    let fdu_len = u.int_in_range(2..=41)?;
    let mut items = Vec::new();
    for _ in 0..u.int_in_range(1..=14)? {
        if u.arbitrary()? {
            let m = if u.arbitrary()? { 1 } else { 3 };
            items.push(Item::Sdu(m, octets(u, MAX_SDU)?));
        } else {
            let n = u.int_in_range(3..=MAX_SDU)?;
            items.push(Item::Packet(2, packet(n, u.arbitrary()?)));
        }
    }
    let qos_bits = qos_bits(u)?;
    let map = |id: u8| MapEntry::<Tc>::new(MapId::<6>::new(id).unwrap(), SegmenterService::Any, ());
    let maps = [
        map(1).with_segmentation(MAX_SDU).unwrap(),
        map(2).with_segmentation(MAX_SDU).unwrap().with_blocking(),
        map(3).with_segmentation(MAX_SDU).unwrap(),
    ];
    let config = MapSegmenterConfig::<Tc, 4>::new(
        vc(),
        TcSegmenterParams::with_segment_header(tc_physical(false)),
        fdu_len,
        &maps,
    )
    .unwrap();
    let mut seg = MapSegmenter::<Tc, 4, Q>::new(config).unwrap();
    let mut area = vec![0_u8; AREA];
    for i in &items {
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
        let _ = seg.step(input, T0, &mut area).unwrap();
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
    let mut bits = qos_bits.iter().copied().cycle();
    let mut qos = qos_of(false);
    let mut fresh = true;
    // bound: every FDU takes octets off the queue, which ends; the count stops a defect.
    for _ in 0..10_000 {
        if fresh {
            qos = qos_of(bits.next().unwrap());
        }
        let a = seg
            .step(SegmenterInput::NextFdu { handle: 1, qos }, T0, &mut area)
            .unwrap();
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
        let r = reas.step(input, &mut out).unwrap();
        collect(&r, &out, &mut delivered);
    }
    let want = expected(&items);
    assert_eq!(delivered.events, 0);
    assert_eq!(delivered.sdus, want.sdus);
    assert_eq!(delivered.packets, want.packets);
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "one straight run of the segmenter into the reassembler for the three services"
)]
fn uslp_trip(u: &mut Unstructured<'_>) -> Result<()> {
    let fdu_len = u.int_in_range(2..=41)?;
    let mut items: Vec<(Option<Item>, Vec<u8>)> = Vec::new();
    for _ in 0..u.int_in_range(1..=14)? {
        match u.int_in_range(0..=2)? {
            0 => items.push((Some(Item::Sdu(2, octets(u, MAX_SDU)?)), Vec::new())),
            1 => {
                let n = u.int_in_range(3..=MAX_SDU)?;
                items.push((Some(Item::Packet(1, packet(n, u.arbitrary()?))), Vec::new()));
            }
            _ => items.push((None, octets(u, 30)?)),
        }
    }
    let qos_bits = qos_bits(u)?;
    let map = |id: u8, service, upid: u8| {
        MapEntry::<Uslp>::new(
            MapId::<4>::new(id).unwrap(),
            service,
            Upid::from_bits(upid).unwrap(),
        )
    };
    let two = VcfCountLen::new(2).unwrap();
    let maps = [
        map(1, SegmenterService::Packets, 0)
            .with_segmentation(MAX_SDU)
            .unwrap()
            .with_blocking(),
        map(2, SegmenterService::Sdu, 5)
            .with_segmentation(MAX_SDU)
            .unwrap(),
        map(
            3,
            SegmenterService::OctetStream {
                release_deadline: Duration::from_micros(100),
            },
            4,
        ),
    ];
    let config =
        MapSegmenterConfig::<Uslp, 4>::new(vc(), uslp_params(two, two), fdu_len, &maps).unwrap();
    let mut seg = MapSegmenter::<Uslp, 4, Q>::new(config).unwrap();
    let mut area = vec![0_u8; AREA];
    let mut sent = Vec::new();
    for (item, stream) in &items {
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
        let _ = seg.step(input, T0, &mut area).unwrap();
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
    // bound: every FDU takes octets off the queue; a step with nothing to give and no deadline
    // ends the loop; the count stops a defect.
    for _ in 0..10_000 {
        if fresh {
            qos = qos_of(bits.next().unwrap());
        }
        let at = Instant::from_micros(now);
        let a = seg
            .step(SegmenterInput::NextFdu { handle: 1, qos }, at, &mut area)
            .unwrap();
        let Some(SegmenterOutput::Fdu(f)) = a.get(0).copied() else {
            if let Some(due) = seg.next_deadline() {
                now = due.as_micros();
                let _ = seg
                    .step(
                        SegmenterInput::Timeout,
                        Instant::from_micros(now),
                        &mut area,
                    )
                    .unwrap();
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
        let r = reas.step(input, &mut out).unwrap();
        collect(&r, &out, &mut delivered);
    }
    let mut want = expected(&sent);
    want.stream = items.iter().flat_map(|(_, s)| s.clone()).collect();
    assert_eq!(delivered.events, 0);
    assert_eq!(delivered.sdus, want.sdus);
    assert_eq!(delivered.packets, want.packets);
    assert_eq!(delivered.stream, want.stream);
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    // An input too short for a scenario is skipped.
    let _ = match u.arbitrary() {
        Ok(true) => tc_trip(&mut u),
        Ok(false) => uslp_trip(&mut u),
        Err(e) => Err(e),
    };
});
