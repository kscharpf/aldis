//! Calls every public `aldis-link` API of the MAP engines of M4 WP-6 (VER-010): the `MapSegmenter`
//! and the `MapReassembler` for TC and USLP with their configurations, inputs, refusals, and
//! status, and the `CltuPacker` for TC and USLP. Inputs pass through `black_box`. The `link/`
//! vector files are engine scenarios the host suites run.

use core::hint::black_box as bb;

use aldis_core::ErrorKind;
use aldis_core::ids::{MapId, VcId};
use aldis_core::time::{Duration, Instant};
use aldis_link::cltu::{
    CLTU_PACKER_MAX_ACTIONS, CltuInput, CltuOutput, CltuPacker, CltuPackerConfig, FrameIn,
    Repetitions,
};
use aldis_link::common::Qos;
use aldis_link::extractor::PacketRun;
use aldis_link::protocol::{Tc, Uslp};
use aldis_link::reassembler::{
    FduIn, MAP_REASSEMBLER_MAX_ACTIONS, MapReassembler, MapReassemblerConfig, ReassemblerInput,
    ReassemblerMap, ReassemblerOutput, ReassemblyEvent, ReassemblyService, TcReassemblerParams,
    Unit, UslpFdu,
};
use aldis_link::segmenter::{
    Framing, MAP_SEGMENTER_MAX_ACTIONS, MapEntry, MapSegmenter, MapSegmenterConfig, MapService,
    SegmenterInput, SegmenterOutput, TcSegmenterParams, UslpSegmenterParams,
};
use aldis_uslp::{Upid, VcfCountLen};

use crate::exercise::Tally;

/// The queue of the segmenters, the assembly buffer of the reassemblers, the unit of the packers.
const Q: usize = 96;
const S: usize = 32;
const U: usize = 24;

/// An Encapsulation Packet of 4 octets: the header of 2 octets (PVN 7, length of length 1), the
/// total, and 2 octets of data `fill` (133.1 §4.1.2).
const fn encapsulation(fill: u8) -> [u8; 4] {
    [0xFD, 0x04, fill, fill]
}

/// The `MapSegmenter` and the `MapReassembler` for TC: a segmented SDU, blocked packets, a VC
/// without a Segment Header, the refusals, and `Reconfigure`.
#[expect(
    clippy::too_many_lines,
    reason = "one straight-line call of every public API of the TC engines"
)]
fn tc(t: &mut Tally) {
    let (Some(vc), Some(m1), Some(m2)) = (
        VcId::<6>::new(bb(1)),
        MapId::<6>::new(bb(1)),
        MapId::<6>::new(bb(2)),
    ) else {
        t.check(false);
        return;
    };
    let now = Instant::from_micros(bb(0));
    let Ok(sdu_map) = MapEntry::<Tc>::new(m1, MapService::Any, ()).with_segmentation(bb(40)) else {
        t.check(false);
        return;
    };
    t.check(
        MapEntry::<Tc>::new(m1, MapService::Any, ())
            .with_segmentation(bb(0))
            .is_err(),
    );
    let packet_map = MapEntry::<Tc>::new(m2, MapService::Any, ()).with_blocking();
    t.check(sdu_map.id() == m1 && sdu_map.segmentation() == Some(40) && !sdu_map.blocking());
    t.check(packet_map.blocking() && packet_map.service() == MapService::Any);
    let () = *packet_map.params();
    let channel = TcSegmenterParams::with_segment_header(bb(true));
    t.check(channel.segment_header() && channel.fecf());
    let Ok(config) = MapSegmenterConfig::<Tc, 2>::new(vc, channel, bb(12), &[sdu_map, packet_map])
    else {
        t.check(false);
        return;
    };
    t.check(config.vc() == vc && config.max_fdu_len() == 12 && config.zone_len() == 11);
    t.check(config.channel() == &channel && config.maps().count() == 2);
    t.check(config.map(2) == Some(&packet_map) && config.map(bb(3)).is_none());
    t.check(MapSegmenterConfig::<Tc, 2>::new(vc, channel, bb(1), &[sdu_map]).is_err());
    let Ok(mut seg) = MapSegmenter::<Tc, 2, Q>::new(config) else {
        t.check(false);
        return;
    };
    let mut out = [0u8; 16];
    let sdu = [bb(7u8); 20];
    t.check(
        seg.step(
            SegmenterInput::SubmitSdu { map: m1, sdu: &sdu },
            now,
            &mut out,
        )
        .is_ok_and(|a| a.is_empty()),
    );
    let packet = encapsulation(bb(3));
    let p = packet.as_slice();
    t.check(
        seg.step(
            SegmenterInput::SubmitPacket { map: m2, packet: p },
            now,
            &mut out,
        )
        .is_ok(),
    );
    t.check(
        seg.step(
            SegmenterInput::SubmitPacket { map: m2, packet: p },
            now,
            &mut out,
        )
        .is_ok(),
    );
    t.check(
        seg.step(
            SegmenterInput::SubmitOctets { map: m1, octets: p },
            now,
            &mut out,
        )
        .err()
        .map(|e| e.kind())
            == Some(ErrorKind::InvalidField),
    );
    t.check(
        seg.step(
            SegmenterInput::SubmitSdu { map: m1, sdu: &[] },
            now,
            &mut out,
        )
        .err()
        .map(|e| e.kind())
            == Some(ErrorKind::LengthMismatch),
    );
    t.check(
        seg.step(SegmenterInput::Timeout, now, &mut out)
            .is_ok_and(|a| a.is_empty()),
    );
    t.check(seg.next_deadline().is_none() && MAP_SEGMENTER_MAX_ACTIONS == 1);
    let mut short = [0u8; 4];
    t.check(
        seg.step(
            SegmenterInput::NextFdu {
                handle: 1,
                qos: Qos::SequenceControlled,
            },
            now,
            &mut short,
        )
        .err()
        .map(|e| e.kind())
            == Some(ErrorKind::BufferTooSmall),
    );
    let Ok(rm1) = ReassemblerMap::<Tc>::sdu(vc, m1, bb(S)) else {
        t.check(false);
        return;
    };
    let Ok(rm2) = ReassemblerMap::<Tc>::packets(vc, m2, bb(S)) else {
        t.check(false);
        return;
    };
    let params = TcReassemblerParams::with_segment_header();
    let Ok(rconfig) = MapReassemblerConfig::<Tc, 2>::new(params, &[rm1, rm2]) else {
        t.check(false);
        return;
    };
    t.check(rconfig.channel() == &params && rconfig.maps().count() == 2);
    t.check(rm1.vc() == vc && rm1.id() == m1 && rm1.service() == ReassemblyService::Sdu);
    t.check(rm1.max_sdu_len() == Some(S) && rm2.service() == ReassemblyService::Packets);
    let Ok(mut reas) = MapReassembler::<Tc, 2, S>::new(rconfig) else {
        t.check(false);
        return;
    };
    let mut area = [0u8; 64];
    let mut delivered = 0_usize;
    // bound: three requests, one for each FDU of the queue (a segmented SDU and a block).
    for handle in 0..4 {
        let step = seg.step(
            SegmenterInput::NextFdu {
                handle,
                qos: Qos::SequenceControlled,
            },
            now,
            &mut out,
        );
        let Ok(actions) = step else {
            t.check(false);
            return;
        };
        for o in actions.iter() {
            if let SegmenterOutput::Fdu(f) = o {
                t.check(f.handle == handle && f.vc == 1 && f.qos == Qos::SequenceControlled);
                t.check(matches!(
                    f.framing,
                    Framing::Whole | Framing::First | Framing::Continuing | Framing::Last
                ));
                let data = f.fdu.slice(&out).unwrap_or_default();
                let input = ReassemblerInput::Fdu(FduIn { vc, meta: (), data });
                let Ok(got) = reas.step(input, &mut area) else {
                    t.check(false);
                    return;
                };
                for r in got.iter() {
                    match *r {
                        ReassemblerOutput::Sdu { sdu, .. } => {
                            t.check(sdu.slice(&area).is_some_and(|s| s.len() == 20));
                            delivered = delivered.saturating_add(1);
                        }
                        ReassemblerOutput::Packets { run, count, .. } => {
                            let n = PacketRun::new(run, &area).map_or(0, Iterator::count);
                            t.check(n == count && count == 2);
                            delivered = delivered.saturating_add(1);
                        }
                        ReassemblerOutput::Event { .. }
                        | ReassemblerOutput::StreamData { .. }
                        | ReassemblerOutput::Discarded { .. } => t.check(false),
                    }
                }
            }
        }
    }
    t.check(delivered == 2 && MAP_REASSEMBLER_MAX_ACTIONS == 2);
    let rs = reas.status();
    t.check(rs.fdus_in.get() == 3 && rs.delivered.get() == 3 && rs.assembly.current() == 0);
    t.check(seg.status().fdus_out.get() == 3 && seg.status().submitted.get() == 3);
    // Sequence violation, an unknown MAP, a block that does not tile, and Reconfigure.
    let events = |reas: &mut MapReassembler<Tc, 2, S>, data: &[u8], area: &mut [u8; 64]| {
        reas.step(ReassemblerInput::Fdu(FduIn { vc, meta: (), data }), area)
            .map(|a| {
                a.iter().find_map(|o| {
                    if let ReassemblerOutput::Event { event, .. } = o {
                        Some(*event)
                    } else {
                        None
                    }
                })
            })
    };
    t.check(
        events(&mut reas, &[0x81, 1], &mut area) == Ok(Some(ReassemblyEvent::SequenceViolation)),
    );
    t.check(events(&mut reas, &[0xC9, 1], &mut area) == Ok(Some(ReassemblyEvent::UnknownMap)));
    t.check(events(&mut reas, &[0xC2, 0x00, 0x01], &mut area).is_err());
    t.check(events(&mut reas, &[], &mut area).is_err());
    t.check(
        reas.step(
            ReassemblerInput::Fdu(FduIn {
                vc,
                meta: (),
                data: &[0x41, 1, 2],
            }),
            &mut area,
        )
        .is_ok(),
    );
    t.check(
        reas.step(ReassemblerInput::Reconfigure(rconfig), &mut area)
            .is_ok_and(|a| a.get(0) == Some(&ReassemblerOutput::Discarded { octets: 2 })),
    );
    t.check(reas.config() == &rconfig && reas.status().discarded_octets.get() == 2);
    t.check(
        seg.step(SegmenterInput::Reconfigure(config), now, &mut out)
            .is_ok_and(|a| a.is_empty()),
    );
    t.check(seg.config() == &config && Unit::Idle != Unit::Whole(&[]));
    // A VC without a Segment Header: one implicit MAP 0, the packets and the SDU of the FDU.
    let Some(zero) = MapId::<6>::new(bb(0)) else {
        t.check(false);
        return;
    };
    let plain = TcSegmenterParams::without_segment_header(bb(false), bb(true));
    let Ok(pconfig) = MapSegmenterConfig::<Tc, 1>::new(vc, plain, bb(20), &[]) else {
        t.check(false);
        return;
    };
    let mut pout = [0u8; 20];
    let Ok(mut pseg) = MapSegmenter::<Tc, 1, Q>::new(pconfig) else {
        t.check(false);
        return;
    };
    t.check(
        pseg.step(
            SegmenterInput::SubmitPacket {
                map: zero,
                packet: p,
            },
            now,
            &mut pout,
        )
        .is_ok(),
    );
    t.check(
        pseg.step(
            SegmenterInput::SubmitPacket {
                map: zero,
                packet: p,
            },
            now,
            &mut pout,
        )
        .is_ok(),
    );
    let fdu = pseg.step(
        SegmenterInput::NextFdu {
            handle: 9,
            qos: Qos::Expedited,
        },
        now,
        &mut pout,
    );
    t.check(fdu.is_ok_and(|a| {
        a.get(0).is_some_and(
            |o| matches!(o, SegmenterOutput::Fdu(f) if f.map.is_none() && f.fdu.len() == 8),
        )
    }));
    let Ok(rp) = ReassemblerMap::<Tc>::packets(vc, zero, bb(S)) else {
        t.check(false);
        return;
    };
    let plain_rx = TcReassemblerParams::without_segment_header();
    let Ok(pconfig_rx) = MapReassemblerConfig::<Tc, 1>::new(plain_rx, &[rp]) else {
        t.check(false);
        return;
    };
    let Ok(mut preas) = MapReassembler::<Tc, 1, S>::new(pconfig_rx) else {
        t.check(false);
        return;
    };
    let data = pout.get(..8).unwrap_or_default();
    t.check(
        preas
            .step(
                ReassemblerInput::Fdu(FduIn { vc, meta: (), data }),
                &mut area,
            )
            .is_ok_and(|a| a.len() == 1),
    );
}

/// The `MapSegmenter` and the `MapReassembler` for USLP: segments, a stream with its deadline,
/// fixed-length SDUs by the pointer, the gap, and the refusals.
#[expect(
    clippy::too_many_lines,
    reason = "one straight-line call of every public API of the USLP engines"
)]
fn uslp(t: &mut Tally) {
    let (Some(vc), Some(m1), Some(m2), Some(m3)) = (
        VcId::<6>::new(bb(1)),
        MapId::<4>::new(bb(1)),
        MapId::<4>::new(bb(2)),
        MapId::<4>::new(bb(3)),
    ) else {
        t.check(false);
        return;
    };
    let (Ok(two), Ok(one)) = (VcfCountLen::new(bb(2)), VcfCountLen::new(bb(1))) else {
        t.check(false);
        return;
    };
    let channel = UslpSegmenterParams::new(two, one);
    t.check(channel.vcf_len_sc() == two && channel.vcf_len_exp() == one);
    let Ok(sdus) =
        MapEntry::<Uslp>::new(m1, MapService::Sdu, Upid::OctetStream).with_segmentation(bb(30))
    else {
        t.check(false);
        return;
    };
    let packets = MapEntry::<Uslp>::new(m2, MapService::Packets, Upid::Packets).with_blocking();
    let deadline = Duration::from_micros(bb(500));
    let stream = MapEntry::<Uslp>::new(
        m3,
        MapService::OctetStream {
            release_deadline: deadline,
        },
        Upid::OctetStream,
    );
    t.check(
        MapSegmenterConfig::<Uslp, 3>::new(vc, channel, bb(9), &[stream.with_blocking()]).is_err(),
    );
    t.check(
        MapSegmenterConfig::<Uslp, 3>::new(
            vc,
            UslpSegmenterParams::new(VcfCountLen::NONE, one),
            bb(9),
            &[packets],
        )
        .is_err(),
    );
    let Ok(config) =
        MapSegmenterConfig::<Uslp, 3>::new(vc, channel, bb(9), &[sdus, packets, stream])
    else {
        t.check(false);
        return;
    };
    let Ok(mut seg) = MapSegmenter::<Uslp, 3, Q>::new(config) else {
        t.check(false);
        return;
    };
    let mut out = [0u8; 16];
    let at = |us: u64| Instant::from_micros(bb(us));
    let sdu = [bb(5u8); 20];
    t.check(
        seg.step(
            SegmenterInput::SubmitSdu { map: m1, sdu: &sdu },
            at(0),
            &mut out,
        )
        .is_ok(),
    );
    t.check(
        seg.step(
            SegmenterInput::SubmitOctets {
                map: m3,
                octets: &[1, 2, 3],
            },
            at(10),
            &mut out,
        )
        .is_ok(),
    );
    t.check(seg.next_deadline() == Some(at(510)));
    let mut area = [0u8; 64];
    let rmaps = [
        ReassemblerMap::<Uslp>::sdu(vc, m1, bb(S)),
        ReassemblerMap::<Uslp>::packets(vc, m2, bb(S)),
    ];
    let (Ok(rm1), Ok(rm2)) = (rmaps[0], rmaps[1]) else {
        t.check(false);
        return;
    };
    let rm3 = ReassemblerMap::<Uslp>::octet_stream(vc, m3);
    t.check(rm3.max_sdu_len().is_none() && rm3.service() == ReassemblyService::OctetStream);
    let Ok(rconfig) = MapReassemblerConfig::<Uslp, 3>::new((), &[rm1, rm2, rm3]) else {
        t.check(false);
        return;
    };
    let Ok(mut reas) = MapReassembler::<Uslp, 3, S>::new(rconfig) else {
        t.check(false);
        return;
    };
    let mut seen = 0_u32;
    // bound: the SDU takes three TFDFs, the stream one, after its deadline.
    for handle in 0..6 {
        let now = at(u64::from(handle).saturating_mul(200));
        if handle == 3 {
            t.check(seg.step(SegmenterInput::Timeout, now, &mut out).is_ok());
        }
        let Ok(actions) = seg.step(
            SegmenterInput::NextFdu {
                handle,
                qos: Qos::SequenceControlled,
            },
            now,
            &mut out,
        ) else {
            t.check(false);
            return;
        };
        for o in actions.iter() {
            let SegmenterOutput::Fdu(f) = o else {
                t.check(false);
                continue;
            };
            let Some(map) = f.map.and_then(MapId::<4>::new) else {
                t.check(false);
                continue;
            };
            let data = f.fdu.slice(&out).unwrap_or_default();
            let meta = UslpFdu { map, gap: false };
            let Ok(got) = reas.step(ReassemblerInput::Fdu(FduIn { vc, meta, data }), &mut area)
            else {
                t.check(false);
                return;
            };
            for r in got.iter() {
                match *r {
                    ReassemblerOutput::Sdu { sdu, .. } => {
                        t.check(sdu.slice(&area).is_some_and(|s| s.len() == 20));
                        seen = seen.saturating_add(1);
                    }
                    ReassemblerOutput::StreamData {
                        data, after_gap, ..
                    } => {
                        t.check(data.slice(&area).is_some_and(|s| s.len() == 3) && !after_gap);
                        seen = seen.saturating_add(1);
                    }
                    ReassemblerOutput::Event { .. }
                    | ReassemblerOutput::Packets { .. }
                    | ReassemblerOutput::Discarded { .. } => t.check(false),
                }
            }
        }
    }
    t.check(seen == 2 && seg.status().queue.current() == 0);
    // A gap discards the SDUs in progress of the VC; a fixed-length SDU ends at its pointer.
    let tfdf = |rule: u8, rest: &[u8]| {
        let mut f = [0u8; 8];
        for (d, s) in f.iter_mut().zip([rule << 5 | 5].iter().chain(rest)) {
            *d = *s;
        }
        f
    };
    let go = |reas: &mut MapReassembler<Uslp, 3, S>,
              map: MapId<4>,
              gap,
              f: [u8; 8],
              len: usize,
              area: &mut [u8; 64]| {
        let data = f.get(..len).unwrap_or_default();
        reas.step(
            ReassemblerInput::Fdu(FduIn {
                vc,
                meta: UslpFdu { map, gap },
                data,
            }),
            area,
        )
    };
    let start = tfdf(0b100, &[1, 2]);
    t.check(go(&mut reas, m1, false, start, 3, &mut area).is_ok_and(|a| a.is_empty()));
    let cont = tfdf(0b101, &[3]);
    let r = go(&mut reas, m1, true, cont, 2, &mut area);
    t.check(r.is_ok_and(|a| {
        a.len() == 2
            && a.get(0)
                == Some(&ReassemblerOutput::Event {
                    event: ReassemblyEvent::GapDiscard,
                    vc: 1,
                    map: 1,
                })
    }));
    let fixed = tfdf(0b001, &[0, 1, 9, 8, 7]);
    let r = go(&mut reas, m1, false, fixed, 8, &mut area);
    t.check(r.is_ok_and(|a| a.iter().any(|o| matches!(o, ReassemblerOutput::Sdu { .. }))));
    t.check(
        go(
            &mut reas,
            m1,
            false,
            tfdf(0b001, &[0, 9, 1, 1]),
            6,
            &mut area,
        )
        .is_err(),
    );
    t.check(go(&mut reas, m1, false, tfdf(0b011, &[1]), 2, &mut area).is_err());
    t.check(go(&mut reas, m3, true, tfdf(0b011, &[1]), 2, &mut area).is_ok());
    let unknown = MapId::<4>::new(bb(9));
    t.check(
        unknown.is_some_and(|m| go(&mut reas, m, false, tfdf(0b111, &[1]), 2, &mut area).is_ok()),
    );
    t.check(reas.status().gap_discards.get() == 1 && reas.status().unknown_maps.get() == 1);
    t.check(reas.status().sequence_violations.get() == 1 && reas.status().sdus_too_long.get() == 0);
    t.check(go(&mut reas, m1, false, tfdf(0b111, &[0; 7]), 8, &mut area).is_ok());
    let mut tiny = [0u8; 2];
    t.check(go(&mut reas, m1, false, tfdf(0b111, &[1]), 2, &mut area).is_ok() && tiny.len() == 2);
    t.check(
        reas.step(ReassemblerInput::Reconfigure(rconfig), &mut tiny)
            .is_ok(),
    );
    t.check(
        seg.step(SegmenterInput::Reconfigure(config), at(2_000), &mut out)
            .is_ok_and(|a| a.is_empty()),
    );
}

/// The `CltuPacker` for TC and USLP.
#[expect(
    clippy::too_many_lines,
    reason = "one straight-line call of every public API of the packer"
)]
fn cltu(t: &mut Tally) {
    let (Ok(tc), Ok(uslp)) = (
        CltuPackerConfig::<Tc>::new(bb(2), bb(12)),
        CltuPackerConfig::<Uslp>::new(bb(2), bb(12)),
    ) else {
        t.check(false);
        return;
    };
    t.check(CltuPackerConfig::<Tc>::new(bb(0), bb(12)).is_err());
    t.check(CltuPackerConfig::<Tc>::new(bb(1), bb(0)).is_err());
    t.check(tc.max_frames() == 2 && tc.max_unit_len() == 12);
    t.check(CltuPacker::<Tc, 8>::new(tc).is_err());
    let (Ok(mut tp), Ok(mut up)) = (
        CltuPacker::<Tc, U>::new(tc),
        CltuPacker::<Uslp, U>::new(uslp),
    ) else {
        t.check(false);
        return;
    };
    let Some(rep) = Repetitions::new(bb(2)) else {
        t.check(false);
        return;
    };
    t.check(rep.get() == 2 && Repetitions::ONE.get() == 1 && Repetitions::new(bb(0)).is_none());
    let mut out = [0u8; 2 * U];
    let frame = [bb(4u8); 8];
    let ad = FrameIn {
        frame: &frame,
        qos: Qos::SequenceControlled,
        repetitions: rep,
        truncated: false,
    };
    let bd = FrameIn {
        qos: Qos::Expedited,
        ..ad
    };
    t.check(
        tp.step(CltuInput::AddFrame(ad), &mut out)
            .is_ok_and(|a| a.is_empty()),
    );
    // 8 + 8 > 12: the pending unit is returned; the new frame is pending.
    let r = tp.step(CltuInput::AddFrame(ad), &mut out);
    t.check(r.is_ok_and(|a| {
        a.len() == 1
            && matches!(a.get(0), Some(CltuOutput::DataUnit(u))
                if u.frames == 1 && u.repetitions == rep && !u.truncated && u.unit.len() == 8)
    }));
    // The expedited service asks for no repetition, so it cannot share the unit.
    let r = tp.step(CltuInput::AddFrame(bd), &mut out);
    t.check(r.is_ok_and(|a| a.len() == 1 && CLTU_PACKER_MAX_ACTIONS == 2));
    t.check(
        tp.step(CltuInput::Flush, &mut out)
            .is_ok_and(|a| a.len() == 1),
    );
    t.check(
        tp.step(CltuInput::Flush, &mut out)
            .is_ok_and(|a| a.is_empty()),
    );
    t.check(
        tp.step(
            CltuInput::AddFrame(FrameIn {
                truncated: true,
                ..ad
            }),
            &mut out,
        )
        .err()
        .map(|e| e.kind())
            == Some(ErrorKind::InvalidField),
    );
    t.check(
        tp.step(CltuInput::AddFrame(FrameIn { frame: &[], ..ad }), &mut out)
            .err()
            .map(|e| e.kind())
            == Some(ErrorKind::LengthMismatch),
    );
    t.check(
        tp.step(
            CltuInput::AddFrame(FrameIn {
                frame: &[0; 13],
                ..ad
            }),
            &mut out,
        )
        .err()
        .map(|e| e.kind())
            == Some(ErrorKind::OutOfRange),
    );
    let mut short = [0u8; 3];
    t.check(
        tp.step(CltuInput::AddFrame(ad), &mut short)
            .err()
            .map(|e| e.kind())
            == Some(ErrorKind::BufferTooSmall),
    );
    let truncated = FrameIn {
        truncated: true,
        ..ad
    };
    t.check(
        up.step(CltuInput::AddFrame(truncated), &mut out)
            .is_ok_and(|a| a.is_empty()),
    );
    let r = up.step(
        CltuInput::AddFrame(FrameIn {
            truncated: false,
            ..ad
        }),
        &mut out,
    );
    t.check(r.is_ok_and(
        |a| matches!(a.get(0), Some(CltuOutput::DataUnit(u)) if u.truncated && u.frames == 1),
    ));
    t.check(
        up.step(CltuInput::Reconfigure(uslp), &mut out)
            .is_ok_and(|a| a.get(0) == Some(&CltuOutput::Discarded { octets: 8 })),
    );
    let s = up.status();
    t.check(s.frames_in.get() == 2 && s.units_out.get() == 1 && s.discarded_octets.get() == 8);
    t.check(s.pending.current() == 0 && s.refused.get() == 0 && up.config() == &uslp);
    t.check(tp.status().refused.get() == 3);
}

/// Runs the MAP engines.
pub fn run(t: &mut Tally) {
    tc(t);
    uslp(t);
    cltu(t);
}
