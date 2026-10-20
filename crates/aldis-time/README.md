# aldis-time

Codec for the CCSDS 301.0-B-4 Time Code Formats: CUC, CDS, CCS, the ASCII time codes, agency-defined codes, and conversions between the UTC-based codes.

**Status:** implemented in Milestone 1, release `v0.2.0-m1` ([plan](../../docs/milestones/m1-packets-time.md)). The C ABI functions for this crate follow in Milestone 2, work package 0 ([roadmap](../../docs/08-roadmap.md) §1, OI-21).

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`. `aldis-spp` uses it, behind its feature `time`, to decode a time code in a packet's secondary header.

## What it provides

- P-field parsing (`PField`, `TimeCodeId`) and two decoders: `decode_with_pfield` for a code that carries its P-field, and `decode_implicit` for a T-field whose `TimeFormat` is known from management (301.0 §3.1). Both return a `TimeCode`, which encodes back to octets with or without the P-field.
- One value type per code, each with a format type that fixes its layout: `Cuc` (301.0 §3.2), `Cds` (§3.3), `Ccs` (§3.4), `AsciiTime` for ASCII codes A and B and their subsets (§3.5), and `Agency` for agency-defined codes (§3.6). The values are range-checked when they are constructed or decoded (RS-020).
- Conversions: CUC to and from seconds and nanoseconds, and `UtcInstant` to convert between CDS, CCS, and ASCII (301.0 §3.3–3.5).
- Strict and lenient decoding (`aldis_core::DecodeMode`); a lenient decode reports what it accepted as `TimeAnomaly` flags.

It does not read a clock or a time zone (TIME-004), and it does not convert between time scales. CUC Level 1 counts TAI seconds and takes no leap seconds (TIME-014); CDS and CCS are UTC-based, and leap-second handling is the caller's (TIME-022). A leap-second table is deferred to v1.1 (OI-10).

The module table in [`src/lib.rs`](src/lib.rs) maps every module to its 301.0 clauses and requirement IDs.

## Features

None by default (GEN-005). `alloc` and `std` are strictly additive: no change in behaviour or wire output (GEN-006).

## Example

Encode a Level 1 CUC with its P-field and decode it again:

```rust
use aldis_core::DecodeMode;
use aldis_time::{Cuc, CucEpoch, CucFormat, TimeCode, decode_with_pfield};

fn main() -> Result<(), aldis_core::Error> {
    // 4 coarse octets (seconds) and 2 fine octets (fraction of a second).
    let format = CucFormat::new(CucEpoch::Tai1958, 4, 2)?;
    let cuc = Cuc::from_seconds_nanos(format, 2_000_000_000, 500_000_000)?;

    let mut buf = [0_u8; 16];
    let len = TimeCode::Cuc(cuc).encode(true, &mut buf)?;

    // The P-field identifies the code and its layout.
    let decoded = decode_with_pfield(&buf[..len], DecodeMode::Strict)?;
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

The evidence for the release is the exit table of the [M1 plan](../../docs/milestones/m1-packets-time.md) §4: coverage, fuzzing, mutation testing, vectors on the host, on s390x, and under QEMU, and the cross-checks ([cross-checks.md](../../docs/verification/cross-checks.md)). The bounded-loop inspection is [gen-022-023-aldis-time.md](../../docs/verification/inspections/gen-022-023-aldis-time.md), and the second review of the hand-derived vectors is [m1-vectors.md](../../docs/verification/reviews/m1-vectors.md). The shared conformance vectors are in [`test-vectors/time/`](../../test-vectors/time/).

## Specification

- [docs/standards/time-codes.md](../../docs/standards/time-codes.md): requirement area TIME, with the 301.0 clause for each requirement, the ASCII grammar, and the conversion rules.
- [docs/03-api-model.md](../../docs/03-api-model.md): the codec contract (decode modes, errors) shared by every codec.
- [docs/09-open-issues.md](../../docs/09-open-issues.md): OI-10 (leap seconds, deferred to v1.1); OI-32 (independent implementations not yet run for CUC P-field octet 2, CCS, ASCII, and the conversions).
- [docs/README.md](../../docs/README.md): the index of the specification.
