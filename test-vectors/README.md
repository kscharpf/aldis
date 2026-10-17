# Conformance test vectors

Shared, language-neutral vectors that every implementation must pass: Rust, the C ABI, language bindings, and the future native C/C++ ports. The format is specified in [../docs/07-test-vectors.md](../docs/07-test-vectors.md).

## Seed vectors

| File | Content | Provenance |
|---|---|---|
| `core/crc16.json` | FECF CRC-16 check value (CMN-002) | Hand-derived from the generator definition (132.0 §4.1.6). Computed independently during specification work. |
| `core/oid.json` | First 20 octets of the OID idle sequence (CMN-020) | Blue Book example (132.0 Annex D). Reproduced by the reference recurrence in `docs/standards/common-link-features.md` §3. |

All other directories are created as each milestone adds vectors (see [../docs/08-roadmap.md](../docs/08-roadmap.md)).
