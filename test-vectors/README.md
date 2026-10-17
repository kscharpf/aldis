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

Validate with `cargo xtask vectors validate`. The JSON Schemas in `schema/` are generated from the Rust model (`cargo xtask vectors schema`), so other implementations can validate the same files.

All other directories are created as each milestone adds vectors (see [../docs/08-roadmap.md](../docs/08-roadmap.md)).
