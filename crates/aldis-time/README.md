# aldis-time

Codec for the CCSDS 301.0-B-4 Time Code Formats: CUC, CDS, CCS, the ASCII time codes, agency-defined codes, and conversions between the UTC-based codes.

## Where it sits

Layer 1 (codecs) of the [architecture](../../docs/01-architecture.md) §2. It depends only on `aldis-core`. `aldis-spp` uses it, behind its feature `time`, to decode a time code in a packet's secondary header.

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

A caller with an `Instant` clock converts it with `Cuc::from_instant` (T0 plus the `Instant`), and sizes a buffer from the format alone with `TimeFormat::encoded_len`, so a Space Packet secondary header needs no trial encode:

```rust
use aldis_core::time::Instant;
use aldis_time::{Cuc, CucEpoch, CucFormat, TimeCode, TimeFormat};

fn main() -> Result<(), aldis_core::Error> {
    let format = CucFormat::new(CucEpoch::Tai1958, 4, 2)?;
    // 4 + 2 octets and a one-octet P-field.
    let len = TimeFormat::Cuc(format).encoded_len(true);
    assert_eq!(len, 7);

    // T0 = 2 100 000 000 s, 3.125 s after it.
    let cuc = Cuc::from_instant(format, 2_100_000_000, Instant::from_micros(3_125_000))?;
    assert_eq!(cuc.to_seconds_nanos(), (2_100_000_003, 125_000_000));

    let mut buf = [0_u8; 7];
    assert_eq!(TimeCode::Cuc(cuc).encode(true, &mut buf)?, len);
    Ok(())
}
```
