# aldis-core

Foundation types and shared algorithms of Aldis: the error catalogue, decode modes, bit-field access, range-checked identifiers, the Frame Error Control Field CRC-16, the Operational Control Field, the Only-Idle-Data generator, modular counters, fixed-capacity containers, and the reserved SDLS hooks. Each algorithm that several CCSDS data link protocols share is implemented here once ([common link features](../../docs/standards/common-link-features.md)).

## Where it sits

Layer 0 (foundation) of the [architecture](../../docs/01-architecture.md) §2. It has no dependencies, and every other library crate depends on it.

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
