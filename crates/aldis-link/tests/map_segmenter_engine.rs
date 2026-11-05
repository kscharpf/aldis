//! Configuration, refusals, bounds, deadlines, and the choices of the `MapSegmenter` that the
//! vectors do not pin (LNK-002, LNK-003, LNK-012, LNK-040…LNK-043, LNK-074, LNK-075, LNK-078,
//! API-052, API-053, API-058; OI-78).
#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::arithmetic_side_effects,
    unused_must_use,
    reason = "tests fail loudly by design, and many steps are run for their effect on the state"
)]

use aldis_core::ErrorKind;
use aldis_core::field;
use aldis_core::ids::{MapId, VcId};
use aldis_core::time::{Duration, Instant};
use aldis_link::common::Qos;
use aldis_link::protocol::{Tc, Uslp};
use aldis_link::segmenter::{
    Framing, MAP_SEGMENTER_MAX_ACTIONS, MapEntry, MapSegmenter, MapSegmenterConfig, MapService,
    SegmentedFdu, SegmenterInput, SegmenterOutput, TcSegmenterParams, UslpSegmenterParams,
};
use aldis_uslp::{Upid, VcfCountLen};

const T0: Instant = Instant::from_micros(0);
const SC: Qos = Qos::SequenceControlled;
const EXP: Qos = Qos::Expedited;

type TcCfg = MapSegmenterConfig<Tc, 4>;
type TcSeg = MapSegmenter<Tc, 4, 128>;
type UslpCfg = MapSegmenterConfig<Uslp, 4>;
type UslpSeg = MapSegmenter<Uslp, 4, 128>;

const fn tc_map(id: u8) -> MapEntry<Tc> {
    MapEntry::<Tc>::new(MapId::<6>::new(id).unwrap(), MapService::Any, ())
}

const fn vc() -> VcId<6> {
    VcId::<6>::new(1).unwrap()
}

/// A TC VC with FDUs of 16 octets: MAP 1 segments (60 octets), MAP 2 blocks.
fn tc_config() -> TcCfg {
    TcCfg::new(
        vc(),
        TcSegmenterParams::with_segment_header(true),
        16,
        &[
            tc_map(1).with_segmentation(60).unwrap(),
            tc_map(2).with_blocking(),
        ],
    )
    .unwrap()
}

const fn uslp_map(id: u8, service: MapService, upid: u8) -> MapEntry<Uslp> {
    MapEntry::<Uslp>::new(
        MapId::<4>::new(id).unwrap(),
        service,
        Upid::from_bits(upid).unwrap(),
    )
}

fn counts(sc: u8, exp: u8) -> UslpSegmenterParams {
    UslpSegmenterParams::new(
        VcfCountLen::new(sc).unwrap(),
        VcfCountLen::new(exp).unwrap(),
    )
}

/// A USLP VC with TFDFs of 12 octets (11 of TFDZ): MAP 2 SDUs of 40 octets, MAP 4 a stream.
fn uslp_config() -> UslpCfg {
    UslpCfg::new(
        vc(),
        counts(2, 2),
        12,
        &[
            uslp_map(2, MapService::Sdu, 5)
                .with_segmentation(40)
                .unwrap(),
            uslp_map(
                4,
                MapService::OctetStream {
                    release_deadline: Duration::from_micros(500),
                },
                4,
            ),
        ],
    )
    .unwrap()
}

/// A Space Packet of `total` octets (at least 7), APID 1, data `fill`.
fn spp(total: usize, fill: u8) -> Vec<u8> {
    let mut p = vec![0x00, 0x01, 0xC0, 0x00];
    p.extend_from_slice(&u16::try_from(total - 7).unwrap().to_be_bytes());
    p.resize(total, fill);
    p
}

fn kind_of<T>(r: &Result<T, aldis_core::Error>) -> (ErrorKind, Option<aldis_core::FieldId>) {
    let e = r.as_ref().err().unwrap();
    (e.kind(), e.field())
}

fn run<P: aldis_link::segmenter::SegmenterProtocol, const M: usize, const Q: usize>(
    s: &mut MapSegmenter<P, M, Q>,
    input: SegmenterInput<'_, P, M>,
) -> Result<(), aldis_core::Error> {
    let mut out = [0_u8; 64];
    s.step(input, T0, &mut out).map(|_| ())
}

fn submit_sdu<P: aldis_link::segmenter::SegmenterProtocol, const M: usize, const Q: usize>(
    s: &mut MapSegmenter<P, M, Q>,
    map: u8,
    sdu: &[u8],
) -> Result<(), aldis_core::Error> {
    let mut out = [0_u8; 64];
    s.step(
        SegmenterInput::SubmitSdu {
            map: P::Map::try_from(map).unwrap(),
            sdu,
        },
        T0,
        &mut out,
    )
    .map(|_| ())
}

fn next<P: aldis_link::segmenter::SegmenterProtocol, const M: usize, const Q: usize>(
    s: &mut MapSegmenter<P, M, Q>,
    qos: Qos,
    at: u64,
) -> Result<Option<(SegmentedFdu, Vec<u8>)>, aldis_core::Error> {
    let mut out = [0_u8; 64];
    let a = s.step(
        SegmenterInput::NextFdu { handle: 7, qos },
        Instant::from_micros(at),
        &mut out,
    )?;
    Ok(a.get(0).map(|o| match o {
        SegmenterOutput::Fdu(f) => (*f, f.fdu.slice(&out).unwrap().to_vec()),
        other @ SegmenterOutput::Discarded { .. } => panic!("{other:?}"),
    }))
}

#[test]
fn configurations_are_checked() {
    // req: LNK-078, LNK-040, API-040
    let header = TcSegmenterParams::with_segment_header(true);
    let bad = |len| {
        TcCfg::new(vc(), header, len, &[tc_map(1)])
            .err()
            .map(|e| (e.kind(), e.field()))
    };
    assert_eq!(
        bad(1),
        Some((ErrorKind::InvalidConfig, Some(field::MAX_FDU_LEN)))
    );
    assert_eq!(
        bad(1018),
        Some((ErrorKind::InvalidConfig, Some(field::MAX_FDU_LEN)))
    );
    assert_eq!(bad(2), None);
    assert_eq!(bad(1017), None);
    let plain = TcSegmenterParams::with_segment_header(false);
    assert!(header.segment_header() && header.fecf());
    assert!(plain.segment_header() && !plain.fecf());
    assert!(!TcSegmenterParams::without_segment_header(true, true).segment_header());
    assert!(TcSegmenterParams::without_segment_header(true, true).fecf());
    assert!(!TcSegmenterParams::without_segment_header(false, true).fecf());
    assert!(TcCfg::new(vc(), plain, 1019, &[tc_map(1)]).is_ok());
    assert!(TcCfg::new(vc(), plain, 1020, &[tc_map(1)]).is_err());
    // A TC MAP that permits segmentation needs a maximum MAP_SDU length (232.0 Table 5-4).
    let e = tc_map(1).with_segmentation(0).unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidConfig, Some(field::MAX_SDU_LEN))
    );
    assert!(tc_map(1).with_segmentation(65_535).is_ok());
    assert!(tc_map(1).with_segmentation(65_536).is_err());
    // MAPs: none, twice, too many; a MAP on a VC without a Segment Header.
    let map_id = Some((ErrorKind::InvalidConfig, Some(field::MAP_ID)));
    let cfg = |maps: &[MapEntry<Tc>]| {
        TcCfg::new(vc(), header, 16, maps)
            .err()
            .map(|e| (e.kind(), e.field()))
    };
    assert_eq!(cfg(&[]), map_id);
    assert_eq!(cfg(&[tc_map(3), tc_map(3)]), map_id);
    let five = [tc_map(1), tc_map(2), tc_map(3), tc_map(4), tc_map(5)];
    assert_eq!(
        cfg(&five),
        Some((ErrorKind::CapacityExceeded, Some(field::MAP_ID)))
    );
    let bare = TcSegmenterParams::without_segment_header(false, true);
    let e = TcCfg::new(vc(), bare, 16, &[tc_map(1)]).unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidConfig, Some(field::MAP_ID))
    );
    let bare_cfg = TcCfg::new(vc(), bare, 16, &[]).unwrap();
    assert_eq!(bare_cfg.maps().count(), 1);
    assert!(bare_cfg.maps().all(|m| m.blocking() && m.id().get() == 0));
    assert_eq!(bare_cfg.zone_len(), 16);
    // A stream is a USLP service, and it neither blocks nor segments.
    let stream = MapService::OctetStream {
        release_deadline: Duration::from_micros(1),
    };
    let e = TcCfg::new(
        vc(),
        header,
        16,
        &[MapEntry::<Tc>::new(MapId::<6>::new(1).unwrap(), stream, ())],
    )
    .unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::InvalidConfig, Some(field::DATA_FIELD_CONTENT))
    );
    let uslp = |m: MapEntry<Uslp>| {
        UslpCfg::new(vc(), counts(1, 1), 12, &[m])
            .err()
            .map(|e| (e.kind(), e.field()))
    };
    let stream_map = uslp_map(1, stream, 4);
    assert_eq!(uslp(stream_map), None);
    assert_eq!(
        uslp(stream_map.with_blocking()),
        Some((ErrorKind::InvalidConfig, Some(field::BLOCKING)))
    );
    assert_eq!(
        uslp(stream_map.with_segmentation(9).unwrap()),
        Some((ErrorKind::InvalidConfig, Some(field::SEGMENTATION)))
    );
    // The UPID "Only Idle Data" labels no user data.
    assert_eq!(
        uslp(uslp_map(1, MapService::Packets, 31)),
        Some((ErrorKind::InvalidConfig, Some(field::UPID)))
    );
    // A USLP VC that segments, or a MAP that blocks, needs a VCF Count (LNK-078).
    let counted = uslp_map(1, MapService::Packets, 0);
    let without = |m: MapEntry<Uslp>| {
        UslpCfg::new(vc(), counts(0, 1), 12, &[m])
            .err()
            .map(|e| (e.kind(), e.field()))
    };
    let no_count = Some((ErrorKind::InvalidConfig, Some(field::VCF_COUNT_LEN)));
    assert_eq!(without(counted), None);
    assert_eq!(without(counted.with_blocking()), no_count);
    assert_eq!(without(counted.with_segmentation(20).unwrap()), no_count);
    // The frame holds the TFDF Header and one octet.
    assert!(UslpCfg::new(vc(), counts(1, 1), 1, &[counted]).is_err());
    assert!(UslpCfg::new(vc(), counts(1, 1), 2, &[counted]).is_ok());
    assert!(UslpCfg::new(vc(), counts(1, 1), 65_536, &[counted]).is_err());
}

#[test]
fn the_queue_must_hold_the_longest_sdu() {
    // req: LNK-073, LNK-015, GEN-011
    let long = tc_map(1).with_segmentation(125).unwrap();
    let cfg = |m: MapEntry<Tc>| {
        TcCfg::new(
            vc(),
            TcSegmenterParams::with_segment_header(false),
            16,
            &[m],
        )
        .unwrap()
    };
    // 125 octets and the 4 of the entry fit 129? No: Q = 128.
    let e = TcSeg::new(cfg(long)).unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::CapacityExceeded, Some(field::MAX_SDU_LEN))
    );
    assert!(TcSeg::new(cfg(tc_map(1).with_segmentation(124).unwrap())).is_ok());
    // A stream needs a TFDZ in the queue.
    let stream = MapService::OctetStream {
        release_deadline: Duration::from_micros(1),
    };
    let wide = UslpCfg::new(vc(), counts(1, 1), 200, &[uslp_map(1, stream, 4)]).unwrap();
    let e = UslpSeg::new(wide).unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::CapacityExceeded, Some(field::MAX_FDU_LEN))
    );
    // Reconfigure checks the same and changes nothing.
    let mut s = TcSeg::new(tc_config()).unwrap();
    submit_sdu(&mut s, 1, &[1, 2, 3]).unwrap();
    let mut out = [0_u8; 64];
    let r = s.step(SegmenterInput::Reconfigure(cfg(long)), T0, &mut out);
    assert_eq!(
        kind_of(&r),
        (ErrorKind::CapacityExceeded, Some(field::MAX_SDU_LEN))
    );
    assert_eq!(next(&mut s, SC, 0).unwrap().unwrap().1, [0xC1, 1, 2, 3]);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one straight list of the refusals of a submission, in the order of the checks"
)]
fn submissions_are_checked() {
    // req: LNK-040, LNK-042, LNK-010, API-052, API-058
    let mut s = TcSeg::new(tc_config()).unwrap();
    let step = run::<Tc, 4, 128>;
    let map = |n: u8| MapId::<6>::new(n).unwrap();
    // An unknown MAP names no field.
    let r = step(
        &mut s,
        SegmenterInput::SubmitSdu {
            map: map(9),
            sdu: &[1],
        },
    );
    assert_eq!(kind_of(&r), (ErrorKind::UnknownChannel, None));
    // A TC MAP is not a stream.
    let r = step(
        &mut s,
        SegmenterInput::SubmitOctets {
            map: map(1),
            octets: &[1],
        },
    );
    assert_eq!(
        kind_of(&r),
        (ErrorKind::InvalidField, Some(field::DATA_FIELD_CONTENT))
    );
    let r = step(
        &mut s,
        SegmenterInput::SubmitSdu {
            map: map(1),
            sdu: &[],
        },
    );
    assert_eq!(
        kind_of(&r),
        (ErrorKind::LengthMismatch, Some(field::SDU_LEN))
    );
    // Packets: the PVN, the length field, and the sizes.
    let r = step(
        &mut s,
        SegmenterInput::SubmitPacket {
            map: map(1),
            packet: &[0x20, 0, 0, 0, 0, 0, 0],
        },
    );
    assert_eq!(
        kind_of(&r),
        (ErrorKind::InvalidField, Some(field::PACKET_VERSION))
    );
    let mut bad = spp(10, 0);
    bad.push(0);
    let r = step(
        &mut s,
        SegmenterInput::SubmitPacket {
            map: map(1),
            packet: &bad,
        },
    );
    assert_eq!(
        kind_of(&r),
        (ErrorKind::LengthMismatch, Some(field::PACKET_LEN))
    );
    let (p61, p60, p16) = (spp(61, 0), spp(60, 0), spp(16, 0));
    let r = step(
        &mut s,
        SegmenterInput::SubmitPacket {
            map: map(1),
            packet: &p61,
        },
    );
    assert_eq!(kind_of(&r).0, ErrorKind::OutOfRange);
    assert_eq!(kind_of(&r).1, Some(field::MAX_SDU_LEN));
    assert!(
        step(
            &mut s,
            SegmenterInput::SubmitPacket {
                map: map(1),
                packet: &p60
            }
        )
        .is_ok()
    );
    // MAP 2 does not segment: 16 octets with the Segment Header exceed the FDU.
    let r = step(
        &mut s,
        SegmenterInput::SubmitPacket {
            map: map(2),
            packet: &p16,
        },
    );
    assert_eq!(
        kind_of(&r),
        (ErrorKind::OutOfRange, Some(field::MAX_FDU_LEN))
    );
    // Nothing of the refusals is queued; the counters count them.
    assert_eq!(s.status().refused.get(), 7);
    assert_eq!(s.status().submitted.get(), 1);
    // A full queue takes nothing and changes nothing (API-052): 64 and 59 octets leave 5, which
    // hold one octet of data and the four of its entry.
    let mut s = TcSeg::new(tc_config()).unwrap();
    for n in [60, 55] {
        step(
            &mut s,
            SegmenterInput::SubmitSdu {
                map: map(1),
                sdu: &vec![7; n],
            },
        )
        .unwrap();
    }
    let r = step(
        &mut s,
        SegmenterInput::SubmitSdu {
            map: map(1),
            sdu: &[7; 2],
        },
    );
    assert_eq!(kind_of(&r), (ErrorKind::WouldBlock, None));
    assert_eq!(s.status().would_block.get(), 1);
    assert_eq!(s.status().queue.current(), 115);
    assert!(
        step(
            &mut s,
            SegmenterInput::SubmitSdu {
                map: map(1),
                sdu: &[7; 1]
            }
        )
        .is_ok()
    );
    assert_eq!(s.status().queue.high_water(), 116);
}

#[test]
fn a_usl_service_serves_its_own_content() {
    // req: LNK-042, LNK-040
    let mut s = UslpSeg::new(
        UslpCfg::new(
            vc(),
            counts(2, 2),
            12,
            &[
                uslp_map(1, MapService::Packets, 0),
                uslp_map(2, MapService::Sdu, 5),
            ],
        )
        .unwrap(),
    )
    .unwrap();
    let map = |n: u8| MapId::<4>::new(n).unwrap();
    let wrong = (ErrorKind::InvalidField, Some(field::DATA_FIELD_CONTENT));
    let packet = spp(8, 0);
    let r = run(
        &mut s,
        SegmenterInput::SubmitSdu {
            map: map(1),
            sdu: &[1],
        },
    );
    assert_eq!(kind_of(&r), wrong);
    let r = run(
        &mut s,
        SegmenterInput::SubmitPacket {
            map: map(2),
            packet: &packet,
        },
    );
    assert_eq!(kind_of(&r), wrong);
    let r = run(
        &mut s,
        SegmenterInput::SubmitOctets {
            map: map(2),
            octets: &[1],
        },
    );
    assert_eq!(kind_of(&r), wrong);
    // Without segmentation an SDU longer than the TFDZ is refused.
    let r = run(
        &mut s,
        SegmenterInput::SubmitSdu {
            map: map(2),
            sdu: &[0; 12],
        },
    );
    assert_eq!(
        kind_of(&r),
        (ErrorKind::OutOfRange, Some(field::MAX_FDU_LEN))
    );
}

#[test]
fn next_fdu_needs_the_whole_area_and_changes_nothing_without_it() {
    // req: LNK-075, API-058
    let mut s = TcSeg::new(tc_config()).unwrap();
    let mut short = [0_u8; 15];
    // Even with nothing queued, the area is checked.
    let r = s.step(
        SegmenterInput::NextFdu { handle: 1, qos: SC },
        T0,
        &mut short,
    );
    assert_eq!(kind_of(&r).0, ErrorKind::BufferTooSmall);
    submit_sdu(&mut s, 1, &[9; 20]).unwrap();
    let r = s.step(
        SegmenterInput::NextFdu { handle: 1, qos: SC },
        T0,
        &mut short,
    );
    assert_eq!(kind_of(&r).0, ErrorKind::BufferTooSmall);
    let (f, octets) = next(&mut s, SC, 0).unwrap().unwrap();
    assert_eq!((f.framing, octets.len()), (Framing::First, 16));
    assert_eq!((f.handle, f.vc, f.map, f.qos), (7, 1, Some(1), SC));
    assert_eq!(octets[0], 0x41);
}

#[test]
fn no_action_exceeds_the_bound() {
    // req: LNK-074, API-057
    assert_eq!(MAP_SEGMENTER_MAX_ACTIONS, 1);
    let mut s = TcSeg::new(tc_config()).unwrap();
    let mut out = [0_u8; 64];
    let mut most = 0;
    let mut count = |a: Result<aldis_link::segmenter::SegmenterActions, aldis_core::Error>| {
        most = most.max(a.unwrap().len());
    };
    count(s.step(
        SegmenterInput::SubmitSdu {
            map: MapId::<6>::new(1).unwrap(),
            sdu: &[1; 30],
        },
        T0,
        &mut out,
    ));
    count(s.step(SegmenterInput::Timeout, T0, &mut out));
    count(s.step(SegmenterInput::NextFdu { handle: 1, qos: SC }, T0, &mut out));
    assert_eq!(most, MAP_SEGMENTER_MAX_ACTIONS);
    // The worst case of `Reconfigure`: one Discarded for the SDU in progress and the queue.
    let a = s
        .step(SegmenterInput::Reconfigure(tc_config()), T0, &mut out)
        .unwrap();
    assert_eq!(a.len(), MAP_SEGMENTER_MAX_ACTIONS);
    assert_eq!(a.get(0), Some(&SegmenterOutput::Discarded { octets: 15 }));
    assert_eq!(s.status().discarded_octets.get(), 15);
    // Nothing held: no action.
    let a = s
        .step(SegmenterInput::Reconfigure(tc_config()), T0, &mut out)
        .unwrap();
    assert!(a.is_empty());
}

#[test]
fn the_qos_must_not_change_while_an_sdu_is_in_progress() {
    // req: LNK-041, LNK-043, API-058
    let mut s = TcSeg::new(tc_config()).unwrap();
    submit_sdu(&mut s, 1, &[5; 40]).unwrap();
    submit_sdu(&mut s, 1, &[6; 3]).unwrap();
    assert_eq!(
        next(&mut s, SC, 0).unwrap().unwrap().0.framing,
        Framing::First
    );
    let r = next(&mut s, EXP, 1);
    assert_eq!(kind_of(&r), (ErrorKind::InvalidField, Some(field::QOS)));
    assert_eq!(
        next(&mut s, SC, 2).unwrap().unwrap().0.framing,
        Framing::Continuing
    );
    assert_eq!(
        next(&mut s, SC, 3).unwrap().unwrap().0.framing,
        Framing::Last
    );
    // The next SDU may use the other service.
    assert_eq!(next(&mut s, EXP, 4).unwrap().unwrap().0.qos, EXP);
    assert!(next(&mut s, EXP, 5).unwrap().is_none());
}

#[test]
fn an_fdu_needs_a_vcf_count_of_its_qos() {
    // req: LNK-078, LNK-042
    // The expedited service has no VCF Count here: it can carry whole SDUs, not portions.
    let cfg = UslpCfg::new(
        vc(),
        counts(2, 0),
        12,
        &[uslp_map(1, MapService::Packets, 0)
            .with_blocking()
            .with_segmentation(40)
            .unwrap()],
    )
    .unwrap();
    let mut s = UslpSeg::new(cfg).unwrap();
    let mut out = [0_u8; 64];
    let mut put = |s: &mut UslpSeg, p: &[u8]| {
        s.step(
            SegmenterInput::SubmitPacket {
                map: MapId::<4>::new(1).unwrap(),
                packet: p,
            },
            T0,
            &mut out,
        )
        .unwrap();
    };
    put(&mut s, &spp(20, 1));
    put(&mut s, &[0xFD, 4, 2, 2]);
    put(&mut s, &[0xFD, 4, 3, 3]);
    let refused = Some((ErrorKind::InvalidField, Some(field::QOS)));
    // The first packet needs segmentation: refused on the expedited service, nothing changes.
    assert_eq!(
        next(&mut s, EXP, 0)
            .as_ref()
            .err()
            .map(|e| (e.kind(), e.field())),
        refused
    );
    let (f, _) = next(&mut s, SC, 1).unwrap().unwrap();
    assert_eq!(f.framing, Framing::First);
    next(&mut s, SC, 2).unwrap().unwrap();
    // Two packets would share one TFDF, which needs the count too (LNK-078: a MAP that blocks).
    assert_eq!(
        next(&mut s, EXP, 3)
            .as_ref()
            .err()
            .map(|e| (e.kind(), e.field())),
        refused
    );
    let (f, octets) = next(&mut s, SC, 4).unwrap().unwrap();
    assert_eq!((f.framing, octets.len()), (Framing::Whole, 9));
    // A lone whole packet is fine on the expedited service.
    put(&mut s, &[0xFD, 4, 4, 4]);
    let (f, _) = next(&mut s, EXP, 5).unwrap().unwrap();
    assert_eq!(f.framing, Framing::Whole);
}

#[test]
fn a_tc_vc_without_a_segment_header_has_one_implicit_map() {
    // req: LNK-040, LNK-078
    let cfg = TcCfg::new(
        vc(),
        TcSegmenterParams::without_segment_header(false, false),
        12,
        &[],
    )
    .unwrap();
    let mut s = TcSeg::new(cfg).unwrap();
    // No blocking: one packet for each FDU. MAP 0 is the only MAP.
    submit_sdu(&mut s, 0, &spp(8, 1)).unwrap();
    assert_eq!(
        kind_of(&submit_sdu(&mut s, 1, &[1])),
        (ErrorKind::UnknownChannel, None)
    );
    let mut out = [0_u8; 64];
    s.step(
        SegmenterInput::SubmitPacket {
            map: MapId::<6>::new(0).unwrap(),
            packet: &spp(7, 2),
        },
        T0,
        &mut out,
    )
    .unwrap();
    let (f, octets) = next(&mut s, SC, 0).unwrap().unwrap();
    assert_eq!((f.map, f.framing, octets.len()), (None, Framing::Whole, 8));
    let (_, octets) = next(&mut s, SC, 1).unwrap().unwrap();
    assert_eq!(octets, spp(7, 2));
    // 13 octets exceed the 12 of the FDU.
    assert_eq!(
        kind_of(&submit_sdu(&mut s, 0, &[0; 13])),
        (ErrorKind::OutOfRange, Some(field::MAX_FDU_LEN))
    );
}

#[test]
fn a_released_timeout_is_harmless() {
    // req: API-053, LNK-012
    let mut s = UslpSeg::new(uslp_config()).unwrap();
    let mut out = [0_u8; 64];
    let map = MapId::<4>::new(4).unwrap();
    let at = |us| Instant::from_micros(us);
    assert_eq!(s.next_deadline(), None);
    s.step(
        SegmenterInput::SubmitOctets {
            map,
            octets: &[1, 2, 3],
        },
        at(100),
        &mut out,
    )
    .unwrap();
    assert_eq!(s.next_deadline(), Some(at(600)));
    // Early and repeated timeouts change nothing.
    for t in [0, 100, 599, 599] {
        s.step(SegmenterInput::Timeout, at(t), &mut out).unwrap();
        assert_eq!(s.next_deadline(), Some(at(600)));
    }
    // More octets join the entry and do not restart the TFDZ.
    s.step(
        SegmenterInput::SubmitOctets { map, octets: &[4] },
        at(300),
        &mut out,
    )
    .unwrap();
    assert_eq!(s.next_deadline(), Some(at(600)));
    assert!(next(&mut s, SC, 599).unwrap().is_none());
    s.step(SegmenterInput::Timeout, at(600), &mut out).unwrap();
    assert_eq!(s.next_deadline(), None);
    let (f, octets) = next(&mut s, SC, 600).unwrap().unwrap();
    assert_eq!((f.framing, f.map), (Framing::Stream, Some(4)));
    assert_eq!(octets, [0x64, 1, 2, 3, 4]);
    s.step(SegmenterInput::Timeout, at(900), &mut out).unwrap();
    assert_eq!(s.next_deadline(), None);
}

#[test]
fn a_stream_that_waits_does_not_hold_the_sdus_behind_it_and_does_not_interrupt_one() {
    // req: LNK-041, LNK-012, LNK-042 (732.1 §4.1.4.2.2.1.6)
    // 732.1 §4.2.5.3 leaves the order to the project; the maintainer accepted this one (OI-79,
    // 2026-11-05; LNK-041 in 04).
    let mut s = UslpSeg::new(uslp_config()).unwrap();
    let mut out = [0_u8; 64];
    let at = |us| Instant::from_micros(us);
    s.step(
        SegmenterInput::SubmitOctets {
            map: MapId::<4>::new(4).unwrap(),
            octets: &[1, 2],
        },
        at(0),
        &mut out,
    )
    .unwrap();
    submit_sdu(&mut s, 2, &[7; 14]).unwrap();
    // The SDU goes first although it came later: the stream TFDZ is not ready.
    let (f, _) = next(&mut s, SC, 1).unwrap().unwrap();
    assert_eq!((f.map, f.framing), (Some(2), Framing::First));
    // The deadline passes while the SDU is in progress; the SDU is not interrupted.
    s.step(SegmenterInput::Timeout, at(500), &mut out).unwrap();
    assert_eq!(s.next_deadline(), None);
    let (f, _) = next(&mut s, SC, 501).unwrap().unwrap();
    assert_eq!((f.map, f.framing), (Some(2), Framing::Last));
    // Now the due TFDZ.
    let (f, octets) = next(&mut s, SC, 502).unwrap().unwrap();
    assert_eq!(
        (f.map, f.framing, octets),
        (Some(4), Framing::Stream, vec![0x64, 1, 2])
    );
    assert!(next(&mut s, SC, 503).unwrap().is_none());
}

#[test]
fn two_streams_report_the_earlier_deadline_and_octets_join_their_entry() {
    // req: LNK-012, API-053
    let stream = |us| MapService::OctetStream {
        release_deadline: Duration::from_micros(us),
    };
    let cfg = UslpCfg::new(
        vc(),
        counts(1, 1),
        6,
        &[uslp_map(1, stream(300), 4), uslp_map(2, stream(100), 4)],
    )
    .unwrap();
    let mut s = UslpSeg::new(cfg).unwrap();
    let mut out = [0_u8; 64];
    let at = |us| Instant::from_micros(us);
    let put = |s: &mut UslpSeg, out: &mut [u8], map: u8, octets: &[u8], us| {
        s.step(
            SegmenterInput::SubmitOctets {
                map: MapId::<4>::new(map).unwrap(),
                octets,
            },
            at(us),
            out,
        )
        .unwrap();
    };
    put(&mut s, &mut out, 1, &[1], 0);
    put(&mut s, &mut out, 2, &[2], 50);
    // MAP 1 again, behind the entry of MAP 2 in the queue; the octets join MAP 1's entry.
    put(&mut s, &mut out, 1, &[3], 60);
    assert_eq!(s.next_deadline(), Some(at(150)));
    s.step(SegmenterInput::Timeout, at(150), &mut out).unwrap();
    assert_eq!(s.next_deadline(), Some(at(300)));
    let (f, octets) = next(&mut s, SC, 150).unwrap().unwrap();
    assert_eq!((f.map, octets), (Some(2), vec![0x64, 2]));
    // Fill MAP 1's TFDZ (5 octets): full, so no deadline and ready.
    put(&mut s, &mut out, 1, &[4, 5, 6, 7], 160);
    assert_eq!(s.next_deadline(), None);
    let (f, octets) = next(&mut s, SC, 170).unwrap().unwrap();
    assert_eq!((f.map, octets), (Some(1), vec![0x64, 1, 3, 4, 5, 6]));
    // The octet that did not fit starts the next TFDZ at the request.
    assert_eq!(s.next_deadline(), Some(at(470)));
    assert_eq!(s.status().fdus_out.get(), 2);
    assert_eq!(s.status().queue.current(), 1);
}

#[test]
fn a_stream_entry_cannot_outgrow_its_length_or_the_queue() {
    // req: API-052, LNK-042
    let stream = MapService::OctetStream {
        release_deadline: Duration::from_micros(1),
    };
    let cfg = UslpCfg::new(vc(), counts(1, 1), 6, &[uslp_map(1, stream, 4)]).unwrap();
    let mut s = MapSegmenter::<Uslp, 4, 70_000>::new(cfg).unwrap();
    let mut out = [0_u8; 8];
    let map = MapId::<4>::new(1).unwrap();
    let big = vec![0_u8; 65_535];
    let r = s.step(
        SegmenterInput::SubmitOctets {
            map,
            octets: &vec![0; 65_536],
        },
        T0,
        &mut out,
    );
    assert_eq!(
        kind_of(&r),
        (ErrorKind::OutOfRange, Some(field::MAX_SDU_LEN))
    );
    s.step(
        SegmenterInput::SubmitOctets { map, octets: &big },
        T0,
        &mut out,
    )
    .unwrap();
    // The entry cannot count one more octet: the stream is backed up.
    let r = s.step(
        SegmenterInput::SubmitOctets { map, octets: &[1] },
        T0,
        &mut out,
    );
    assert_eq!(kind_of(&r), (ErrorKind::WouldBlock, None));
    assert_eq!(s.status().would_block.get(), 1);
    // Taking a TFDZ makes room.
    let a = s
        .step(SegmenterInput::NextFdu { handle: 1, qos: SC }, T0, &mut out)
        .unwrap();
    assert_eq!(a.len(), 1);
    assert!(
        s.step(
            SegmenterInput::SubmitOctets { map, octets: &[1] },
            T0,
            &mut out
        )
        .is_ok()
    );
}

#[test]
fn the_accessors_report_the_configuration() {
    // req: API-056, LNK-078
    let tc = TcSeg::new(tc_config()).unwrap();
    let c = tc.config();
    assert_eq!((c.vc(), c.max_fdu_len()), (vc(), 16));
    assert!(c.channel().segment_header() && c.channel().fecf());
    assert_eq!(c.map(1).map(MapEntry::service), Some(MapService::Any));
    let uslp = UslpSeg::new(uslp_config()).unwrap();
    let ch = uslp.config().channel();
    assert_eq!((ch.vcf_len_sc().get(), ch.vcf_len_exp().get()), (2, 2));
}

// ---- OI-79: cases that no vector pins, read against the Blue Books ----

#[test]
fn only_the_pvns_of_the_two_packet_standards_are_packets() {
    // req: LNK-040, LNK-042, LNK-010, API-058; OI-79 (7)
    // 133.0 §4.1.3.2.2 gives Space Packets the PVN `000` and 133.1 §4.1.2.2.2 gives
    // Encapsulation Packets `111`; no Blue Book the engines serve gives another PVN a length
    // field, so such a packet cannot be delimited. The Blue Books name no error, so the
    // catalogue's `InvalidField` names the field (03 §9); the submission changes nothing.
    let refused = (ErrorKind::InvalidField, Some(field::PACKET_VERSION));
    let mut tc = TcSeg::new(tc_config()).unwrap();
    let cfg = UslpCfg::new(
        vc(),
        counts(1, 1),
        12,
        &[uslp_map(1, MapService::Packets, 0)],
    )
    .unwrap();
    let mut us = UslpSeg::new(cfg).unwrap();
    for pvn in 1..=6_u8 {
        let packet = [pvn << 5, 0, 0, 0, 0, 0, 0, 0];
        let r = run(
            &mut tc,
            SegmenterInput::SubmitPacket {
                map: MapId::<6>::new(1).unwrap(),
                packet: &packet,
            },
        );
        assert_eq!(kind_of(&r), refused, "TC, PVN {pvn}");
        let r = run(
            &mut us,
            SegmenterInput::SubmitPacket {
                map: MapId::<4>::new(1).unwrap(),
                packet: &packet,
            },
        );
        assert_eq!(kind_of(&r), refused, "USLP, PVN {pvn}");
    }
    assert_eq!(tc.status().submitted.get(), 0);
    assert_eq!(us.status().submitted.get(), 0);
    assert_eq!(tc.status().refused.get(), 6);
    assert_eq!(us.status().refused.get(), 6);
    // PVN 0 and 7 are accepted, on TC and on USLP.
    submit_packet(&mut tc, &spp(8, 1)).unwrap();
    submit_packet(&mut tc, &[0xE5, 0x03, 7]).unwrap();
    for packet in [spp(8, 1), vec![0xE5, 0x03, 7]] {
        let r = run(
            &mut us,
            SegmenterInput::SubmitPacket {
                map: MapId::<4>::new(1).unwrap(),
                packet: &packet,
            },
        );
        assert!(r.is_ok(), "{packet:?}");
    }
    assert_eq!(us.status().submitted.get(), 2);
}

fn submit_packet(s: &mut TcSeg, packet: &[u8]) -> Result<(), aldis_core::Error> {
    run(
        s,
        SegmenterInput::SubmitPacket {
            map: MapId::<6>::new(1).unwrap(),
            packet,
        },
    )
}

#[test]
fn a_tc_vc_without_a_segment_header_is_limited_by_the_data_field_and_never_segments() {
    // req: LNK-040, LNK-078; OI-79 (7)
    // 232.0 §4.1.3.1.2: the Frame Data Field is at most 1019 octets (1017 with a FECF), and with
    // no Segment Header it is the FDU itself (§4.1.3.2.1.1 a); §4.1.3.2.1.3: such an FDU holds
    // complete packets or a complete VCA_SDU, so a longer SDU cannot be divided (§4.1.3.2.1.4: a
    // VC that sends SDUs larger than one frame needs the Segment Header).
    for (fecf, limit) in [(false, 1019), (true, 1017)] {
        let params = TcSegmenterParams::without_segment_header(fecf, false);
        assert!(TcCfg::new(vc(), params, limit, &[]).is_ok(), "fecf {fecf}");
        let e = TcCfg::new(vc(), params, limit + 1, &[]).unwrap_err();
        assert_eq!(
            (e.kind(), e.field()),
            (ErrorKind::InvalidConfig, Some(field::MAX_FDU_LEN))
        );
    }
    let cfg = TcCfg::new(
        vc(),
        TcSegmenterParams::without_segment_header(true, false),
        1017,
        &[],
    )
    .unwrap();
    let mut s = TcSegmenter1017::new(cfg).unwrap();
    let mut out = [0_u8; 1017];
    // A VCA_SDU of the limit is one FDU; one octet more is refused, not divided.
    s.step(
        SegmenterInput::SubmitSdu {
            map: MapId::<6>::new(0).unwrap(),
            sdu: &[3; 1017],
        },
        T0,
        &mut out,
    )
    .unwrap();
    let a = s
        .step(SegmenterInput::NextFdu { handle: 1, qos: SC }, T0, &mut out)
        .unwrap();
    assert_eq!(a.len(), 1);
    let Some(SegmenterOutput::Fdu(f)) = a.get(0) else {
        panic!("{a:?}")
    };
    assert_eq!((f.map, f.framing), (None, Framing::Whole));
    assert_eq!(f.fdu.slice(&out).unwrap(), &[3; 1017][..]);
    let a = s
        .step(SegmenterInput::NextFdu { handle: 2, qos: SC }, T0, &mut out)
        .unwrap();
    assert_eq!(a.len(), 0);
    let r = s.step(
        SegmenterInput::SubmitSdu {
            map: MapId::<6>::new(0).unwrap(),
            sdu: &[3; 1018],
        },
        T0,
        &mut out,
    );
    assert_eq!(
        kind_of(&r),
        (ErrorKind::OutOfRange, Some(field::MAX_FDU_LEN))
    );
}

type TcSegmenter1017 = MapSegmenter<Tc, 4, 1100>;
