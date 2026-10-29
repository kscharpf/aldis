# aldis-core

Foundation types and shared algorithms of Aldis: the error catalogue, decode modes, bit-field access, range-checked identifiers, the Frame Error Control Field CRC-16, the Operational Control Field, the Only-Idle-Data generator, modular counters, fixed-capacity containers, and the reserved SDLS hooks. Each algorithm that several CCSDS data link protocols share is implemented here once ([common link features](../../docs/standards/common-link-features.md)).

## Where it sits

Layer 0 (foundation) of the [architecture](../../docs/01-architecture.md) §2. It has no dependencies, and every other library crate depends on it.

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

The evidence is in the exit tables of the [M0 plan](../../docs/milestones/m0-foundations.md) §4 and the [M1 plan](../../docs/milestones/m1-packets-time.md) §4 (coverage, fuzzing, mutation testing, vectors on every target), the bounded-loop analysis record [GEN-022](../../docs/verification/records/GEN-022.md), the [mutants triage](../../docs/verification/mutants-triage.md), and the [analysis records](../../docs/verification/analysis-records.md). The shared conformance vectors are in [`test-vectors/core/`](../../test-vectors/core/).

## Specification

- [docs/standards/common-link-features.md](../../docs/standards/common-link-features.md): requirement area CMN (CRC-16, OCF, OID, counters, field placement with SDLS).
- [docs/02-design-rules.md](../../docs/02-design-rules.md) and [docs/03-api-model.md](../../docs/03-api-model.md): the GEN and API requirements this crate implements, including the error catalogue.
- [docs/09-open-issues.md](../../docs/09-open-issues.md): OI-27 (resolved: numeric field codes, added in M2 WP-0a).
- [docs/README.md](../../docs/README.md): the index of the specification.
