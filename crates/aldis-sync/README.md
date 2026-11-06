# aldis-sync

TM synchronization of CCSDS 131.0-B-6 §9 and §10: the Attached Sync Marker (ASM) and the Channel Access Data Unit (CADU), the two pseudo-random sequences of the randomizer, and a sans-I/O engine that finds the ASM at any bit offset in a stream of octets and returns the unit that follows it.

## Example

```rust
use aldis_sync::{
    ASM_LEN, AsmErrors, AsmSearch, RandomizerSequence, SearchInput, SyncConfig, SyncProtocol,
    attach_asm, max_actions, randomize,
};

let config = SyncConfig::new(SyncProtocol::Tm, 4, AsmErrors::NONE).unwrap();

// Sending end: randomize the frame, then attach the ASM (the marker is never randomized).
let mut frame = [0x01, 0x02, 0x03, 0x04];
randomize(RandomizerSequence::Period131071, &mut frame);
let mut cadu = [0_u8; 8];
assert_eq!(attach_asm(&config, &frame, &mut cadu), Ok(ASM_LEN + 4));

// Receiving end: search the stream, then derandomize the unit the engine returns.
let mut engine = AsmSearch::<4, 16, { max_actions(16) }>::new(config).unwrap();
let mut out = [0_u8; AsmSearch::<4, 16, { max_actions(16) }>::MAX_OUT_LEN];
let actions = engine.step(SearchInput::Chunk(&cadu), &mut out).unwrap();
let action = actions.get(0).unwrap();
let mut unit = [0_u8; 4];
unit.copy_from_slice(action.unit().slice(&out).unwrap());
randomize(RandomizerSequence::Period131071, &mut unit);
assert_eq!(unit, [0x01, 0x02, 0x03, 0x04]);
assert_eq!((action.errors().get(), action.skipped_bits()), (0, 0));
```

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2, with one step engine, `AsmSearch`. It depends only on `aldis-core`. The specification is [`docs/standards/tm-sync.md`](../../docs/standards/tm-sync.md) (SYNC-001…SYNC-020); its vectors are in [`test-vectors/sync/`](../../test-vectors/sync/README.md). The engine only searches: it holds no lock, does not check the spacing of the units, and does not derandomize (SYNC-006, SYNC-019). Error-correcting codes, slicing, and the longer markers of Turbo and LDPC coding are out of v1.

Status: implemented (M5 WP-3). The C ABI follows in WP-4.
