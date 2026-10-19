//! Segmentation and reassembly: an arbitrary SDU segmented and reassembled comes back intact;
//! an arbitrary packet sequence (reordered, dropped, or repeated segments, other APIDs) never
//! panics, never gets the reassembler stuck, and every completed SDU fits the buffer (SPP-013,
//! OI-30).
#![no_main]

// req: SPP-013, GEN-020, VER-022

use aldis_core::DecodeMode;
use aldis_core::ids::{Apid, SeqCount};
use aldis_spp::{
    Discard, PacketType, Reassembler, Segmenter, SequenceCounter, SpacePacket, SppConfig, Status,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

fn run(input: &mut Unstructured<'_>) -> Result<()> {
    let apid = Apid::new(input.int_in_range(0..=0x7FE)?).unwrap();
    let max_user = usize::from(input.int_in_range(1_u8..=32)?);
    let start = SeqCount::new(input.int_in_range(0..=SeqCount::MAX)?).unwrap();
    let cap = usize::from(input.int_in_range(0_u16..=300)?);
    let sdu_len = usize::from(input.int_in_range(1_u16..=256)?);
    let sdu = input.bytes(sdu_len.min(input.len()))?.to_vec();
    if sdu.is_empty() {
        return Ok(());
    }

    // Segment, keep the packets, and reassemble them in order.
    let mut seg = Segmenter::new(
        &SppConfig::DEFAULT,
        PacketType::Telemetry,
        apid,
        &sdu,
        max_user,
    )
    .unwrap();
    let mut counter = SequenceCounter::new(start);
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut out = [0_u8; 64];
    while let Some(len) = seg.next_packet(&mut counter, &[], &mut out).unwrap() {
        packets.push(out[..len].to_vec());
    }
    assert_eq!(packets.len(), sdu.len().div_ceil(max_user));
    let mut buf = vec![0_u8; cap];
    let mut ra = Reassembler::new(apid, &mut buf);
    let mut last = None;
    for raw in &packets {
        let p = SpacePacket::decode(raw, DecodeMode::Strict).unwrap();
        last = Some(ra.push(&p.header(), p.data_field()).unwrap());
    }
    let last = last.unwrap();
    if sdu.len() <= cap {
        assert_eq!(last.status, Status::Complete(sdu.len()));
        assert_eq!(ra.sdu(), sdu.as_slice());
    } else {
        // Once the SDU overflows, the rest of it are orphans.
        assert!(matches!(last.status, Status::Dropped(_)));
    }

    // Replay a scrambled selection of the packets: nothing panics, and outcomes are consistent.
    ra.reset();
    while !input.is_empty() {
        let Some(raw) = packets.get(usize::from(input.arbitrary::<u8>()?) % packets.len()) else {
            break;
        };
        let p = SpacePacket::decode(raw, DecodeMode::Strict).unwrap();
        let outcome = ra.push(&p.header(), p.data_field()).unwrap();
        match outcome.status {
            Status::Complete(n) => assert!(n <= cap && ra.sdu().len() == n && !ra.is_assembling()),
            Status::InProgress => assert!(ra.is_assembling() && ra.sdu().is_empty()),
            Status::Dropped(why) => {
                assert!(matches!(why, Discard::Orphan | Discard::Overflow));
                assert!(!ra.is_assembling() && ra.sdu().is_empty());
            }
        }
        if let Some(d) = outcome.discarded {
            assert!(!matches!(d, Discard::Orphan));
        }
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});
