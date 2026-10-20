# aldis-core

Foundation types and shared algorithms of Aldis: the error catalogue, decode modes, bit-field access, range-checked identifiers, the Frame Error Control Field CRC-16, the Only-Idle-Data generator, modular counters, fixed-capacity containers, and the reserved SDLS hooks. Each algorithm that several CCSDS data link protocols share is implemented here once ([common link features](../../docs/standards/common-link-features.md)).

**Status:** implemented in Milestone 0, release `v0.1.0-m0` ([plan](../../docs/milestones/m0-foundations.md)). Milestone 1 added the decode mode and the anomaly set ([M1 plan](../../docs/milestones/m1-packets-time.md) WP-0, release `v0.2.0-m1`). Milestone 2, work package 0, adds the numeric field codes to `FieldId` (API-082, OI-27; [roadmap](../../docs/08-roadmap.md) §1).

## Where it sits

Layer 0 (foundation) of the [architecture](../../docs/01-architecture.md) §2. It has no dependencies, and every other library crate depends on it.

## What it provides

- Errors: `Error` with an `ErrorKind` from the stable catalogue of [03 API model](../../docs/03-api-model.md) §9, an optional `FieldId`, and `ErrorDetail` values, all without allocation.
- Decoding policy: `DecodeMode` (strict or lenient) and `AnomalySet`, the flags a lenient decode returns. Each codec crate declares its own flag enum.
- Bit fields: big-endian read and write primitives that every codec uses for wire fields.
- Identifiers: range-checked types such as `Apid`, `SeqCount`, `PacketVersion`, `SpacecraftId`, `VcId`, and `MapId`. A value of one of these types is always in range, so an encoder cannot write an out-of-range field.
- Shared link algorithms: the FECF CRC-16 (CMN-001…CMN-004), the OID pseudo-noise generator (CMN-020…CMN-023), modular counters with gap classification (CMN-030, CMN-031), and the frame field layout calculator (CMN-040, CMN-041).
- Resources: fixed-capacity queues and vectors sized by const generics, and `Instant` and `Duration`, a caller-supplied monotonic time (API-003).
- SDLS: `SecurityLayout` and `SecurityHook`, which reserve the Security Header and Trailer of 355.0-B-2 so that SDLS can be added later without breaking the API. v1 implements no cryptography (API-070…API-075).

It reads no clock: time is always passed in by the caller.

The module table in [`src/lib.rs`](src/lib.rs) maps every module to its requirement IDs; each module's documentation cites the CCSDS clauses.

## Features

None by default (GEN-005).

| Feature | Effect |
|---|---|
| `crc-table` | CRC-16 from a 256-entry table in read-only memory instead of the default bitwise loop. Both give the same results (CMN-004) |
| `test-hooks` | A test-only security hook that writes fixed patterns (API-073). Never enable it in a flight build |
| `alloc`, `std` | Strictly additive: no change in behaviour or wire output (GEN-006) |

## Example

Range-checked identifiers, and the FECF of a frame:

```rust
use aldis_core::crc::{verify_fecf, write_fecf};
use aldis_core::ids::Apid;
use aldis_core::ErrorKind;

fn main() -> Result<(), aldis_core::Error> {
    // An identifier exists only if it is in range.
    assert!(Apid::new(Apid::MAX).is_some_and(Apid::is_idle));
    assert!(Apid::new(Apid::MAX + 1).is_none());

    // The FECF covers every octet but the last two, and is written into those two.
    let mut frame = [0x12, 0x34, 0x56, 0x78, 0x00, 0x00];
    write_fecf(&mut frame)?;
    verify_fecf(&frame)?;

    // A corrupted frame is detected.
    frame[0] ^= 0x01;
    assert_eq!(verify_fecf(&frame).map_err(|e| e.kind()), Err(ErrorKind::CrcMismatch));
    Ok(())
}
```

## Guarantees

- `no_std`, no heap allocation, and no `unsafe` (GEN-010, GEN-070).
- No panic on any input (GEN-020). Checked by the bare-metal panic-freedom link check in [`exerciser/`](../../exerciser/) (VER-010) and by the core fuzz targets in [`fuzz/`](../../fuzz/fuzz_targets/) (VER-022).
- No clock, randomness, I/O, or global state (GEN-031, GEN-032).

The evidence is in the exit tables of the [M0 plan](../../docs/milestones/m0-foundations.md) §4 and the [M1 plan](../../docs/milestones/m1-packets-time.md) §4 (coverage, fuzzing, mutation testing, vectors on every target), the bounded-loop inspection [gen-022-023-aldis-core.md](../../docs/verification/inspections/gen-022-023-aldis-core.md), the [mutants triage](../../docs/verification/mutants-triage.md), and the [analysis records](../../docs/verification/analysis-records.md). The shared conformance vectors are in [`test-vectors/core/`](../../test-vectors/core/).

## Specification

- [docs/standards/common-link-features.md](../../docs/standards/common-link-features.md): requirement area CMN (CRC-16, OID, counters, field placement with SDLS).
- [docs/02-design-rules.md](../../docs/02-design-rules.md) and [docs/03-api-model.md](../../docs/03-api-model.md): the GEN and API requirements this crate implements, including the error catalogue.
- [docs/09-open-issues.md](../../docs/09-open-issues.md): OI-27 (resolved: numeric field codes, added in M2).
- [docs/README.md](../../docs/README.md): the index of the specification.
