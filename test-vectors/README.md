# Conformance test vectors

Shared, language-neutral vectors that every implementation must pass: Rust, the C ABI, language bindings, and the future native C/C++ ports. The format is specified in [../docs/07-test-vectors.md](../docs/07-test-vectors.md).

## Seed vectors

| File | Content | Provenance |
|---|---|---|
| `core/crc16.json` | FECF CRC-16 check value (CMN-002) | Hand-derived from the generator definition (132.0 §4.1.6). Computed independently during specification work. |
| `core/oid.json` | First 20 octets of the OID idle sequence (CMN-020) | Blue Book example (132.0 Annex D). Reproduced by the reference recurrence in `docs/standards/common-link-features.md` §3. |
| `core/errors.json` | Stable numeric code of every error kind, and its C ABI value (GEN-040, CABI-020) | Hand-derived from the catalogue in `docs/03-api-model.md` §9 |
| `core/bits.json` | Big-endian bit-field get/set, including unaligned 64-bit fields and out-of-range errors (GEN-050, GEN-051) | Hand-derived; values from an independent Python big-integer reference |
| `core/counters.json` | Modular increment and gap classification for 1- to 64-bit counters (CMN-030, CMN-031) | Hand-derived from CMN-031; independent Python reference |
| `time/{pfield,cuc,cds,ccs}.json` | P-field chains and reserved IDs, agency-defined codes, CUC (including nanosecond conversions), CDS, and CCS decode/encode and errors (TIME-001…TIME-006, TIME-010…TIME-013, TIME-020, TIME-021, TIME-030, TIME-034) | Hand-derived from 301.0-B-4 §3.1–3.4, §3.6, and Annex A; values from an independent Python model of the Blue Book text, written before the Rust codec |
| `time/ascii.json` | ASCII time codes A and B: parse, format, subsets, and errors (TIME-031, TIME-032; OI-29) | Hand-derived from 301.0-B-4 §3.5 and [time-codes.md §5.1](../docs/standards/time-codes.md); cases 001 and 002 are the Blue Book examples; values from an independent Python model written before the Rust code |
| `time/convert.json` | Conversions between CDS, CCS, and ASCII, including the leap second, agency epochs, truncation, and range errors (TIME-033; OI-18, OI-29) | Hand-derived from [time-codes.md §5.2](../docs/standards/time-codes.md); values from the same independent model |
| `epp/*.json` | Encapsulation Packet decode, `peek_length`, encode, idle, `Auto` header-size boundaries, errors, and managed parameters (EPP-001…EPP-012) | Hand-derived from 133.1-B-3 §4.1 and Table 5-1; values from an independent Python model of the Blue Book text, written before the Rust codec |
| `core/layout.json` | Frame field byte ranges with and without secondary header, security, OCF, and FECF (CMN-040, CMN-041, API-075) | Hand-derived from 132.0/232.0/732.0/732.1 §4.1 and 355.0-B-2 §4.1 |

Validate with `cargo xtask vectors validate`. The JSON Schemas in `schema/` are generated from the Rust model (`cargo xtask vectors schema`), so other implementations can validate the same files.

All other directories are created as each milestone adds vectors (see [../docs/08-roadmap.md](../docs/08-roadmap.md)).
