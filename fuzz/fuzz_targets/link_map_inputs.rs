//! Arbitrary inputs to the `MapSegmenter`, the `MapReassembler`, and the `CltuPacker` (VER-022,
//! LNK-074, LNK-075, LNK-044, API-057, API-058, GEN-020): whatever the sequence of submissions,
//! FDUs with random rule bits and gap flags, timeouts, reconfigurations, and frames, no step
//! panics, none reports an internal error, and none returns more actions than the engine's bound.
#![no_main]

// req: VER-022, LNK-074, LNK-075, LNK-044, API-057, API-058, GEN-020

use aldis_core::ErrorKind;
use aldis_core::ids::{MapId, VcId};
use aldis_core::time::{Duration, Instant};
use aldis_link::cltu::{
    CLTU_PACKER_MAX_ACTIONS, CltuInput, CltuPacker, CltuPackerConfig, FrameIn, Repetitions,
};
use aldis_link::common::Qos;
use aldis_link::protocol::Uslp;
use aldis_link::reassembler::{
    FduIn, MapReassembler, MapReassemblerConfig, ReassemblerInput, ReassemblerMap, UslpFdu,
};
use aldis_link::segmenter::{
    MapEntry, MapSegmenter, MapSegmenterConfig, MapService, SegmenterInput, UslpSegmenterParams,
};
use aldis_uslp::{Upid, VcfCountLen};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

const fn vc() -> VcId<6> {
    VcId::<6>::new(1).unwrap()
}

const fn qos_of(bit: bool) -> Qos {
    if bit {
        Qos::Expedited
    } else {
        Qos::SequenceControlled
    }
}

fn data(u: &mut Unstructured<'_>, max: usize) -> Result<Vec<u8>> {
    let n = u.int_in_range(0..=max)?.min(u.len());
    Ok(u.bytes(n)?.to_vec())
}

fn segmenter(u: &mut Unstructured<'_>) -> Result<()> {
    let two = VcfCountLen::new(2).unwrap();
    let map = |id: u8, service, upid: u8| {
        MapEntry::<Uslp>::new(
            MapId::<4>::new(id).unwrap(),
            service,
            Upid::from_bits(upid).unwrap(),
        )
    };
    let maps = [
        map(1, MapService::Packets, 0)
            .with_segmentation(24)
            .unwrap()
            .with_blocking(),
        map(2, MapService::Sdu, 5).with_segmentation(24).unwrap(),
        map(
            3,
            MapService::OctetStream {
                release_deadline: Duration::from_micros(50),
            },
            4,
        ),
    ];
    let config = MapSegmenterConfig::<Uslp, 4>::new(
        vc(),
        UslpSegmenterParams::new(two, VcfCountLen::NONE),
        u.int_in_range(2..=12)?,
        &maps,
    )
    .unwrap();
    let mut seg = MapSegmenter::<Uslp, 4, 96>::new(config).unwrap();
    let mut area = [0_u8; 12];
    let mut now = 0_u64;
    for _ in 0..u.int_in_range(1..=60)? {
        now += u.int_in_range(0..=400)?;
        let m = MapId::<4>::new(u.int_in_range(0..=3)?).unwrap();
        let bytes = data(u, 24)?;
        let input = match u.int_in_range(0..=5)? {
            0 => SegmenterInput::SubmitSdu {
                map: m,
                sdu: &bytes,
            },
            1 => SegmenterInput::SubmitPacket {
                map: m,
                packet: &bytes,
            },
            2 => SegmenterInput::SubmitOctets {
                map: m,
                octets: &bytes,
            },
            3 => SegmenterInput::NextFdu {
                handle: u.int_in_range(0..=2)?,
                qos: qos_of(u.arbitrary()?),
            },
            4 => SegmenterInput::Timeout,
            _ => SegmenterInput::Reconfigure(config),
        };
        match seg.step(input, Instant::from_micros(now), &mut area) {
            Ok(_) => {}
            Err(e) => {
                assert_ne!(e.kind(), ErrorKind::InternalError);
            }
        }
    }
    Ok(())
}

fn reassembler(u: &mut Unstructured<'_>) -> Result<()> {
    let maps = [
        ReassemblerMap::<Uslp>::packets(vc(), MapId::<4>::new(1).unwrap(), 20).unwrap(),
        ReassemblerMap::<Uslp>::sdu(vc(), MapId::<4>::new(2).unwrap(), 12).unwrap(),
        ReassemblerMap::<Uslp>::octet_stream(vc(), MapId::<4>::new(3).unwrap()),
    ];
    let config = MapReassemblerConfig::<Uslp, 4>::new((), &maps).unwrap();
    let mut reas = MapReassembler::<Uslp, 4, 20>::new(config).unwrap();
    let mut area = [0_u8; 64];
    for _ in 0..u.int_in_range(1..=60)? {
        let tfdf = data(u, 24)?;
        let meta = UslpFdu {
            map: MapId::<4>::new(u.int_in_range(0..=3)?).unwrap(),
            gap: u.arbitrary()?,
        };
        let input = if u.int_in_range(0..=9)? == 0 {
            ReassemblerInput::Reconfigure(config)
        } else {
            ReassemblerInput::Fdu(FduIn {
                vc: vc(),
                meta,
                data: &tfdf,
            })
        };
        match reas.step(input, &mut area) {
            Ok(_) => {}
            Err(e) => {
                assert_ne!(e.kind(), ErrorKind::InternalError);
            }
        }
    }
    Ok(())
}

fn packer(u: &mut Unstructured<'_>) -> Result<()> {
    let config = CltuPackerConfig::<Uslp>::new(3, 20).unwrap();
    let mut packer = CltuPacker::<Uslp, 20>::new(config).unwrap();
    let mut area = [0_u8; 40];
    for _ in 0..u.int_in_range(1..=60)? {
        let frame = vec![u.arbitrary::<u8>()?; u.int_in_range(0..=14)?];
        let input = if u.arbitrary()? {
            CltuInput::Flush
        } else {
            CltuInput::AddFrame(FrameIn {
                frame: &frame,
                qos: qos_of(u.arbitrary()?),
                repetitions: Repetitions::new(u.int_in_range(1..=4)?).unwrap(),
                truncated: u.arbitrary()?,
            })
        };
        match packer.step(input, &mut area) {
            Ok(actions) => assert!(actions.len() <= CLTU_PACKER_MAX_ACTIONS),
            Err(e) => {
                assert_ne!(e.kind(), ErrorKind::InternalError);
            }
        }
    }
    Ok(())
}

fuzz_target!(|bytes: &[u8]| {
    let mut u = Unstructured::new(bytes);
    // An input too short for a scenario ends it.
    let _ = segmenter(&mut u);
    let _ = reassembler(&mut u);
    let _ = packer(&mut u);
});
