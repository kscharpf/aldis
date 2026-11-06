//! Arbitrary streams into the ASM search engine, and the round trip of the codec functions
//! (VER-022, SYNC-003, SYNC-004, SYNC-007…SYNC-011, SYNC-014, SYNC-017, SYNC-018, API-058,
//! GEN-020): whatever the configuration, the stream, and its division into chunks, no step
//! panics or reports an internal error; the units and their fields do not depend on the
//! division; a refused input leaves the engine as it was; and a randomized frame behind its ASM
//! at any bit offset comes back as the frame.
#![no_main]

// req: VER-022, SYNC-003, SYNC-004, SYNC-007, SYNC-009, SYNC-010, SYNC-011, SYNC-014, SYNC-017, SYNC-018, API-058, GEN-020

use aldis_core::ErrorKind;
use aldis_sync::{
    ASM, AsmErrors, AsmSearch, RandomizerSequence, SearchInput, SyncConfig, SyncProtocol,
    attach_asm, max_actions, randomize,
};
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

const FRAME: usize = 64;
const CHUNK: usize = 256;
type Engine = AsmSearch<FRAME, CHUNK, { max_actions(CHUNK) }>;
type Unit = (Vec<u8>, u8, u64);

/// Feeds `stream` in the pieces that `cuts` give (each at most `CHUNK` octets) and returns the
/// units, asserting the contract of every step on the way.
fn run(config: SyncConfig, stream: &[u8], cuts: &[usize]) -> Vec<Unit> {
    let mut engine = Engine::new(config).unwrap();
    let mut units = Vec::new();
    let mut at = 0;
    let mut cuts = cuts.iter().copied();
    while at < stream.len() {
        let want = cuts.next().unwrap_or(CHUNK).clamp(1, CHUNK);
        let end = stream.len().min(at + want);
        let mut out = [0_u8; Engine::MAX_OUT_LEN];
        let before = engine.clone();
        let step = engine.step(SearchInput::Chunk(&stream[at..end]), &mut out);
        let actions = step.unwrap();
        // An area one octet short of what the chunk needs is refused and changes nothing.
        assert!(actions.len() <= max_actions(CHUNK));
        let needed = actions.len() * config.frame_len();
        if needed > 0 {
            let mut probe = before.clone();
            let e = probe
                .step(SearchInput::Chunk(&stream[at..end]), &mut out[..needed - 1])
                .unwrap_err();
            assert_eq!(e.kind(), ErrorKind::BufferTooSmall);
            assert_eq!(probe, before);
        }
        for a in actions.iter() {
            assert!(a.errors() <= config.asm_errors());
            let unit = a.unit().slice(&out).unwrap();
            assert_eq!(unit.len(), config.frame_len());
            units.push((unit.to_vec(), a.errors().get(), a.skipped_bits()));
        }
        at = end;
    }
    units
}

fn config(u: &mut Unstructured<'_>) -> Result<SyncConfig> {
    let protocol = match u.int_in_range(0..=2)? {
        0 => SyncProtocol::Tm,
        1 => SyncProtocol::Aos,
        _ => SyncProtocol::Uslp,
    };
    let len = u.int_in_range(1..=FRAME)?;
    let tolerance = AsmErrors::new(u.int_in_range(0..=15)?).unwrap();
    Ok(SyncConfig::new(protocol, len, tolerance).unwrap())
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let Ok(config) = config(&mut u) else { return };
    let Ok(sequence) = u.arbitrary::<bool>().map(|b| {
        if b {
            RandomizerSequence::Period131071
        } else {
            RandomizerSequence::Period255
        }
    }) else {
        return;
    };
    let Ok(shift) = u.int_in_range(0..=7_usize) else {
        return;
    };
    let Ok(cuts) = u.arbitrary::<Vec<u8>>() else {
        return;
    };
    let cuts: Vec<usize> = cuts.into_iter().map(usize::from).collect();
    let stream = u.take_rest();

    // Any stream: the units do not depend on the division into chunks.
    let a = run(config, stream, &cuts);
    let b = run(config, stream, &[]);
    assert_eq!(a, b);

    // The round trip: randomize, attach, shift to a bit offset, search, derandomize.
    let mut frame = stream
        .iter()
        .copied()
        .cycle()
        .take(config.frame_len())
        .collect::<Vec<u8>>();
    frame.resize(config.frame_len(), 0);
    let mut sent = frame.clone();
    randomize(sequence, &mut sent);
    let mut cadu = vec![0_u8; frame.len() + ASM.len()];
    assert_eq!(attach_asm(&config, &sent, &mut cadu), Ok(cadu.len()));
    let mut bits: Vec<bool> = vec![false; shift];
    bits.extend(
        cadu.iter()
            .flat_map(|o| (0..8).map(move |i| o >> (7 - i) & 1 == 1)),
    );
    let packed: Vec<u8> = bits
        .chunks(8)
        .map(|c| {
            c.iter()
                .enumerate()
                .fold(0, |acc, (i, &bit)| acc | (u8::from(bit) << (7 - i)))
        })
        .collect();
    let units = run(config, &packed, &cuts);
    // The zero bits before the marker can only match it early within the tolerance; the unit
    // that is found first is the frame when none does (tolerance 0 never matches early).
    if config.asm_errors() == AsmErrors::NONE {
        let (mut unit, errors, skipped) = units[0].clone();
        randomize(sequence, &mut unit);
        assert_eq!((unit, errors, skipped), (frame, 0, shift as u64));
    }
});
