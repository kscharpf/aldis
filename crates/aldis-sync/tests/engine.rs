//! The engine contract of `AsmSearch` beyond the vectors: the worst-case bounds, the state
//! left by a refused input, and Reset (SYNC-011…SYNC-015, API-057, API-058, API-062).
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    unused_must_use,
    reason = "tests fail loudly by design, bookkeeping is on lengths the test fixes, and some steps are run for their effect on the state (RS-018)"
)]

use aldis_core::{ErrorDetail, ErrorKind, field};
use aldis_sync::{
    ASM, AsmErrors, AsmSearch, SearchInput, SearchState, SyncConfig, SyncProtocol, max_actions,
    max_out_len,
};

const CHUNK: usize = 20;
type Engine<const F: usize> = AsmSearch<F, CHUNK, { max_actions(CHUNK) }>;

fn config(len: usize, tolerance: u32) -> SyncConfig {
    SyncConfig::new(SyncProtocol::Tm, len, AsmErrors::new(tolerance).unwrap()).unwrap()
}

/// `n` back-to-back CADUs of frames of `len` octets, each frame filled with its number.
fn cadus(n: u8, len: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for i in 0..n {
        v.extend(ASM);
        v.extend(vec![i + 1; len]);
    }
    v
}

#[test]
fn the_worst_case_reaches_the_action_and_area_bounds() {
    // req: SYNC-013, SYNC-014, API-057, API-062
    // Frames of 1 octet: 20 octets hold four CADUs, N = ceil(20 / 5) = 4 actions of 1 octet.
    let mut engine = Engine::<8>::new(config(1, 0)).unwrap();
    let stream = cadus(4, 1);
    assert_eq!(stream.len(), CHUNK);
    let mut out = [0_u8; Engine::<8>::MAX_OUT_LEN];
    let actions = engine.step(SearchInput::Chunk(&stream), &mut out).unwrap();
    assert_eq!(actions.len(), max_actions(CHUNK));
    assert_eq!(max_out_len(1, CHUNK), 4);
    let units: Vec<&[u8]> = actions
        .iter()
        .map(|a| a.unit().slice(&out).unwrap())
        .collect();
    assert_eq!(units, [[1], [2], [3], [4]]);
    // The ranges follow each other without overlap (API-061).
    let starts: Vec<usize> = actions.iter().map(|a| a.unit().start()).collect();
    assert_eq!(starts, [0, 1, 2, 3]);

    // A unit in progress completes first: frames of 8 octets, the chunk ends 3 octets into the
    // next CADU after finishing one that began earlier.
    let mut engine = Engine::<8>::new(config(8, 0)).unwrap();
    let all = cadus(3, 8);
    assert_eq!(
        engine
            .step(SearchInput::Chunk(&all[..6]), &mut out)
            .unwrap()
            .len(),
        0
    );
    assert_eq!(engine.status().state(), SearchState::Collecting);
    assert_eq!(engine.status().partial_octets(), 2);
    let actions = engine
        .step(SearchInput::Chunk(&all[6..26]), &mut out)
        .unwrap();
    assert_eq!(actions.len(), 2);
    assert!(max_out_len(8, CHUNK) <= Engine::<8>::MAX_OUT_LEN);
}

#[test]
fn a_refused_input_changes_nothing() {
    // req: SYNC-014, API-058
    let mut engine = Engine::<4>::new(config(4, 0)).unwrap();
    let stream = cadus(2, 4);
    let mut out = [0_u8; Engine::<4>::MAX_OUT_LEN];
    assert_eq!(
        engine
            .step(SearchInput::Chunk(&stream[..6]), &mut out)
            .unwrap()
            .len(),
        0
    );
    let before = engine.clone();
    // The rest completes two units: 8 octets are needed, and 7 are given.
    let e = engine
        .step(SearchInput::Chunk(&stream[6..]), &mut out[..7])
        .unwrap_err();
    assert_eq!(
        (e.kind(), e.detail()),
        (ErrorKind::BufferTooSmall, ErrorDetail::Needed(8))
    );
    assert_eq!(engine, before);
    // A chunk above CHUNK is OutOfRange naming chunk_len.
    let long = [0_u8; CHUNK + 1];
    let e = engine
        .step(SearchInput::Chunk(&long), &mut out)
        .unwrap_err();
    assert_eq!(
        (e.kind(), e.field()),
        (ErrorKind::OutOfRange, Some(field::CHUNK_LEN))
    );
    assert_eq!(engine, before);
    // The same input is accepted with room.
    let actions = engine
        .step(SearchInput::Chunk(&stream[6..]), &mut out)
        .unwrap();
    assert_eq!(actions.len(), 2);
    // An empty chunk changes nothing and returns no action.
    let after = engine.clone();
    assert!(
        engine
            .step(SearchInput::Chunk(&[]), &mut [])
            .unwrap()
            .is_empty()
    );
    assert_eq!(engine, after);
}

#[test]
fn reset_keeps_the_counters_and_takes_a_new_configuration() {
    // req: SYNC-015, SYNC-002, API-058
    let mut engine = Engine::<8>::new(config(2, 0)).unwrap();
    let mut out = [0_u8; Engine::<8>::MAX_OUT_LEN];
    let stream = cadus(1, 2);
    engine.step(SearchInput::Chunk(&stream), &mut out).unwrap();
    engine
        .step(SearchInput::Chunk(&stream[..5]), &mut out)
        .unwrap();
    assert_eq!(engine.state(), SearchState::Collecting);
    assert_eq!(engine.status().partial_octets(), 1);
    let s = engine.status();
    assert_eq!((s.units_returned(), s.skipped_bits_total()), (1, 0));
    // A longer frame length is refused above FRAME, with the state left alone.
    let before = engine.clone();
    let too_long = config(9, 0);
    let e = engine
        .step(
            SearchInput::Reset {
                config: Some(too_long),
            },
            &mut out,
        )
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::CapacityExceeded);
    assert_eq!(engine, before);
    // Reset with a new configuration: the unit in progress is gone, the counters stay.
    let longer = config(4, 3);
    let actions = engine
        .step(
            SearchInput::Reset {
                config: Some(longer),
            },
            &mut out,
        )
        .unwrap();
    assert!(actions.is_empty());
    assert_eq!(*engine.config(), longer);
    assert_eq!(engine.status().units_returned(), 1);
    assert_eq!(engine.status().partial_octets(), 0);
    let stream = cadus(1, 4);
    let actions = engine.step(SearchInput::Chunk(&stream), &mut out).unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(engine.status().units_returned(), 2);
}

#[test]
fn the_instance_refuses_a_frame_length_above_its_capacity() {
    // req: SYNC-002
    let e = Engine::<4>::new(config(5, 0)).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::CapacityExceeded);
    assert!(Engine::<4>::new(config(4, 0)).is_ok());
}
