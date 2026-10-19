//! `SequenceTracker` against an independent model: per-APID continuity on 14 bits (CMN-031),
//! `First` for a new APID, `CapacityExceeded` without a state change when full, and `forget`
//! (SPP-011, OI-30).
#![no_main]

// req: SPP-011, CMN-031, GEN-011, GEN-024, VER-022

use aldis_core::ErrorKind;
use aldis_core::counter::Continuity;
use aldis_core::ids::{Apid, SeqCount};
use aldis_spp::{SequenceCounter, SequenceTracker, Tracked};
use libfuzzer_sys::fuzz_target;

/// CMN-031 on a 14-bit count, written out independently of `aldis-core`.
fn model_classify(prev: u16, next: u16) -> Continuity {
    let d = (next.wrapping_sub(prev)) & 0x3FFF;
    match d {
        1 => Continuity::InSequence,
        0 => Continuity::Repeat,
        2..=8192 => Continuity::Gap {
            missing: u64::from(d - 1),
        },
        _ => Continuity::Backwards,
    }
}

fuzz_target!(|data: &[u8]| {
    let mut tracker = SequenceTracker::<4>::new();
    let mut model: Vec<(u16, u16)> = Vec::new();
    for chunk in data.as_chunks::<3>().0 {
        let apid = u16::from(chunk[0] & 0x07);
        let count = u16::from_be_bytes([chunk[1], chunk[2]]) & 0x3FFF;
        let a = Apid::new(apid).unwrap();
        if chunk[1] & 0x80 != 0 && chunk[2] == 0 {
            let had = model
                .iter()
                .position(|e| e.0 == apid)
                .map(|i| model.swap_remove(i));
            assert_eq!(tracker.forget(a), had.is_some());
            continue;
        }
        let got = tracker.track(a, SeqCount::new(count).unwrap());
        match model.iter().position(|e| e.0 == apid) {
            Some(at) => {
                assert_eq!(got, Ok(Tracked::Next(model_classify(model[at].1, count))));
                model[at].1 = count;
            }
            None if model.len() == 4 => {
                assert_eq!(got.unwrap_err().kind(), ErrorKind::CapacityExceeded);
            }
            None => {
                assert_eq!(got, Ok(Tracked::First));
                model.push((apid, count));
            }
        }
        assert_eq!(tracker.len(), model.len());
        assert!(
            model
                .iter()
                .all(|e| tracker.last(Apid::new(e.0).unwrap()) == SeqCount::new(e.1))
        );
    }
    // A counter started anywhere yields counts that the tracker sees in sequence.
    let start = data.first().map_or(0, |&b| u16::from(b) * 64);
    let mut counter = SequenceCounter::new(SeqCount::new(start).unwrap());
    let mut fresh = SequenceTracker::<1>::new();
    assert_eq!(
        fresh.track(Apid::IDLE, counter.advance()),
        Ok(Tracked::First)
    );
    for _ in 0..3 {
        assert_eq!(
            fresh.track(Apid::IDLE, counter.advance()),
            Ok(Tracked::Next(Continuity::InSequence))
        );
    }
});
