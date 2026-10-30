# aldis-time

Codec for the CCSDS 301.0-B-4 Time Code Formats: CUC, CDS, CCS, the ASCII time codes, agency-defined codes, and conversions between the UTC-based codes.

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`. `aldis-spp` uses it, behind its feature `time`, to decode a time code in a packet's secondary header.

## Features

None by default (GEN-005). `alloc` and `std` are strictly additive: no change in behaviour or wire output (GEN-006).

## Example

Encode a Level 1 CUC with its P-field and decode it again:

```rust
use aldis_time::{Cuc, CucEpoch, CucFormat, TimeCode, decode_with_pfield};

fn main() -> Result<(), aldis_core::Error> {
    // 4 coarse octets (seconds) and 2 fine octets (fraction of a second).
    let format = CucFormat::new(CucEpoch::Tai1958, 4, 2)?;
    let cuc = Cuc::from_seconds_nanos(format, 2_000_000_000, 500_000_000)?;

    let mut buf = [0_u8; 16];
    let len = TimeCode::Cuc(cuc).encode(true, &mut buf)?;

    // The P-field identifies the code and its layout.
    let decoded = decode_with_pfield(&buf[..len])?;
    assert_eq!(decoded.len(), len);
    assert_eq!(decoded.code(), TimeCode::Cuc(cuc));
    assert_eq!(cuc.to_seconds_nanos(), (2_000_000_000, 500_000_000));
    Ok(())
}
```

## Guarantees

- `no_std`, no heap allocation, and no `unsafe` (GEN-010, GEN-070).
- No panic on any input (GEN-020). Checked by the bare-metal panic-freedom link check in [`exerciser/`](../../exerciser/) (VER-010) and by the fuzz targets `time_*` in [`fuzz/`](../../fuzz/fuzz_targets/) (VER-022).
- No clock, I/O, or global state (GEN-031, GEN-032).

The evidence for the release is the exit table of the [M1 plan](../../docs/milestones/m1-packets-time.md) §4: coverage, fuzzing, mutation testing, vectors on the host, on s390x, and under QEMU, and the cross-checks ([cross-checks.md](../../docs/verification/cross-checks.md)). The bounded-loop analysis record is [GEN-022](../../docs/verification/records/GEN-022.md), and the second review of the hand-derived vectors is [m1-vectors.md](../../docs/verification/reviews/m1-vectors.md). The shared conformance vectors are in [`test-vectors/time/`](../../test-vectors/time/).

## Specification

- [docs/standards/time-codes.md](../../docs/standards/time-codes.md): requirement area TIME, with the 301.0 clause for each requirement, the ASCII grammar, and the conversion rules.
- [docs/03-api-model.md](../../docs/03-api-model.md): the codec contract (decode modes, errors) shared by every codec.
- [docs/09-open-issues.md](../../docs/09-open-issues.md): OI-10 (leap seconds, deferred to v1.1); OI-32 (independent implementations not yet run for CUC P-field octet 2, CCS, ASCII, and the conversions).
- [docs/README.md](../../docs/README.md): the index of the specification.
