//! Property tests of `aldis-sync` (VER-023), drawing octet strings from the shared strategies
//! of `aldis-testkit` (VER-054): the round trip attach, randomize, derandomize, search at every
//! bit offset (SYNC-017), the independence of the result from the chunking (SYNC-010), and the
//! engine against a naive search written from the requirements (SYNC-007…SYNC-009, SYNC-011).
#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::as_conversions,
    reason = "tests fail loudly by design, and the bookkeeping is on lengths the strategies bound (RS-018)"
)]

use aldis_sync::{
    ASM, AsmErrors, AsmSearch, RandomizerSequence, SearchInput, SyncConfig, SyncProtocol,
    attach_asm, max_actions, randomize,
};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::{self, octets};

/// Failing cases are saved here and replayed first.
const REGRESSIONS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/proptest-regressions/properties.txt"
);

const FRAME: usize = 40;
const CHUNK: usize = 256;
type Engine = AsmSearch<FRAME, CHUNK, { max_actions(CHUNK) }>;

fn config(len: usize, tolerance: u32) -> SyncConfig {
    SyncConfig::new(SyncProtocol::Tm, len, AsmErrors::new(tolerance).unwrap()).unwrap()
}

fn bits_of(octets: &[u8]) -> Vec<bool> {
    octets
        .iter()
        .flat_map(|o| (0..8).map(move |i| o >> (7 - i) & 1 == 1))
        .collect()
}

/// The bits packed from the first bit, the last octet padded with zeros.
fn pack(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8)
        .map(|c| {
            c.iter()
                .enumerate()
                .fold(0_u8, |a, (i, &b)| a | (u8::from(b) << (7 - i)))
        })
        .collect()
}

/// The units a naive search finds in `bits`: the first position within the tolerance after 32
/// bits read since the search began, then 8 × `len` bits, then a fresh window.
fn naive(bits: &[bool], len: usize, tolerance: usize) -> Vec<(Vec<u8>, usize, u64)> {
    let asm = bits_of(&ASM);
    let mut found = Vec::new();
    let mut start = 0;
    'units: loop {
        let mut p = start + 32;
        while p <= bits.len() {
            let diff = (0..32).filter(|&i| bits[p - 32 + i] != asm[i]).count();
            if diff <= tolerance {
                if p + 8 * len > bits.len() {
                    break 'units;
                }
                found.push((pack(&bits[p..p + 8 * len]), diff, (p - 32 - start) as u64));
                start = p + 8 * len;
                continue 'units;
            }
            p += 1;
        }
        break;
    }
    found
}

/// Feeds `stream` in the chunks that `cuts` mark and returns the units with their fields.
fn search(config: SyncConfig, stream: &[u8], cuts: &[usize]) -> Vec<(Vec<u8>, usize, u64)> {
    let mut engine = Engine::new(config).unwrap();
    let mut got = Vec::new();
    let mut points: Vec<usize> = cuts.iter().map(|c| c % (stream.len() + 1)).collect();
    points.sort_unstable();
    points.push(stream.len());
    let mut from = 0;
    for to in points {
        let mut at = from;
        while at < to {
            let end = to.min(at + CHUNK);
            let mut out = [0_u8; Engine::MAX_OUT_LEN];
            let actions = engine
                .step(SearchInput::Chunk(&stream[at..end]), &mut out)
                .unwrap();
            for a in actions.iter() {
                got.push((
                    a.unit().slice(&out).unwrap().to_vec(),
                    usize::from(a.errors().get()),
                    a.skipped_bits(),
                ));
            }
            at = end;
        }
        from = to;
    }
    got
}

proptest! {
    #![proptest_config(strategies::config(256, REGRESSIONS))]

    #[test]
    fn attach_randomize_derandomize_search_round_trips(
        frame in octets(1..=FRAME),
        junk_octets in octets(0..4),
        junk_bits in 0_usize..8,
        long in any::<bool>(),
        flips in prop::collection::btree_set(0_usize..32, 0..=15),
        tolerance in 0_u32..=15,
    ) {
        // req: SYNC-017, SYNC-003, SYNC-004, SYNC-006, SYNC-007, SYNC-009, VER-023
        let sequence = if long { RandomizerSequence::Period131071 } else { RandomizerSequence::Period255 };
        let flips: Vec<usize> = flips.into_iter().take(tolerance as usize).collect();
        let cfg = config(frame.len(), tolerance);
        let mut sent = frame.clone();
        randomize(sequence, &mut sent);
        let mut cadu = vec![0_u8; frame.len() + 4];
        prop_assert_eq!(attach_asm(&cfg, &sent, &mut cadu), Ok(frame.len() + 4));
        // The prefix of whole octets and extra bits shifts the CADU to every bit offset.
        let mut junk = bits_of(&junk_octets);
        junk.truncate(junk.len().saturating_sub(8 - junk_bits));
        let mut bits = junk.clone();
        let mut marker = bits_of(&cadu[..4]);
        for &f in &flips { marker[f] = !marker[f]; }
        bits.extend(marker);
        bits.extend(bits_of(&cadu[4..]));
        let stream = pack(&bits);
        // The prefix must not hold a match of its own: the naive search says where the first
        // one is, and the round trip is claimed for the intended position only.
        let expect = naive(&bits, frame.len(), tolerance as usize);
        prop_assume!(expect.len() == 1 && expect[0].2 == junk.len() as u64);
        let units = search(cfg, &stream, &[]);
        prop_assert_eq!(units.len(), 1);
        let (mut unit, errors, skipped) = units[0].clone();
        prop_assert_eq!(errors, flips.len());
        prop_assert_eq!(skipped, junk.len() as u64);
        randomize(sequence, &mut unit);
        prop_assert_eq!(unit, frame);
    }

    #[test]
    fn the_engine_agrees_with_a_naive_search_for_any_chunking(
        data in octets(0..120),
        len in 1_usize..=6,
        tolerance in 0_u32..=15,
        cuts in prop::collection::vec(any::<usize>(), 0..6),
    ) {
        // req: SYNC-007, SYNC-008, SYNC-009, SYNC-010, SYNC-011, VER-023
        let cfg = config(len, tolerance);
        let want = naive(&bits_of(&data), len, tolerance as usize);
        prop_assert_eq!(search(cfg, &data, &cuts), want);
    }

    #[test]
    fn a_stream_of_cadus_gives_every_frame_at_any_chunking(
        frames in prop::collection::vec(octets(3..=3), 1..6),
        junk in prop::collection::vec(any::<bool>(), 0..20),
        cuts in prop::collection::vec(any::<usize>(), 0..8),
    ) {
        // req: SYNC-009, SYNC-010, SYNC-011, VER-023
        let cfg = config(3, 0);
        let mut bits = junk;
        for f in &frames {
            bits.extend(bits_of(&ASM));
            bits.extend(bits_of(f));
        }
        let expect = naive(&bits, 3, 0);
        prop_assume!(expect.len() == frames.len());
        let units = search(cfg, &pack(&bits), &cuts);
        prop_assert_eq!(units, expect);
    }
}

#[test]
fn every_bit_offset_of_a_marker_is_found() {
    // req: SYNC-007, SYNC-009, SYNC-017
    let cfg = config(4, 0);
    for junk in 0..64_usize {
        let frame = [0xA5, 0x5A, 0xFF, 0x00];
        let mut bits = vec![false; junk];
        bits.extend(bits_of(&ASM));
        bits.extend(bits_of(&frame));
        let units = search(cfg, &pack(&bits), &[]);
        assert_eq!(
            units,
            vec![(frame.to_vec(), 0, junk as u64)],
            "{junk} bits of junk"
        );
    }
}
