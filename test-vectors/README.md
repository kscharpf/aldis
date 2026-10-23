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
| `spp/{decode_valid,decode_invalid,stream}.json` | Space Packet decoding: every view field, `peek_length`, each decoder error, the idle anomaly, and packets back to back (SPP-001…SPP-004, SPP-009) | Hand-derived from 133.0-B-2 §4.1 and [space-packet.md §5](../docs/standards/space-packet.md); 133.0 prints no worked example packets; values from an independent Python model written before the Rust codec |
| `spp/{encode,idle,octet_string,split,sequence_tracker}.json` | Encoding with the derived Secondary Header Flag and Maximum Packet Length, Idle Packets, the Octet String service, the opaque secondary header split, and sequence tracking (SPP-005…SPP-007, SPP-010…SPP-012; OI-30) | Same model |
| `spp/{segment,reassemble}.json` | Segmentation flags and counts, and every reassembly rule of §5.4: gap, restart, interruption, orphan, overflow, another APID (SPP-013; OI-30) | Hand-derived from 133.0-B-2 §4.1.3.4.2 and [space-packet.md §5.4](../docs/standards/space-packet.md); values from the same independent model |
| `spp/secondary_header_time.json` | Time secondary headers: CUC and CDS with and without a P-field, ancillary data, the configured time code ID, short data fields, and time code errors (SPP-008; OI-30) | Hand-derived from 133.0-B-2 §4.1.4.2, 301.0-B-4 §3.1–3.3, and [space-packet.md §5.5](../docs/standards/space-packet.md); values from the same independent model |
| `epp/*.json` | Encapsulation Packet decode, `peek_length`, encode, idle, `Auto` header-size boundaries, errors, and managed parameters (EPP-001…EPP-012) | Hand-derived from 133.1-B-3 §4.1 and Table 5-1; values from an independent Python model of the Blue Book text, written before the Rust codec |
| `core/ocf.json` | Operational Control Field classification by bits 0–1 and the checked constructors of the three report types, with field codes (CMN-010, CMN-011) | Hand-derived from 132.0-B-3 §4.1.5.4–5, 732.0-B-5 §4.1.5.4–5, 732.1-B-3 §4.1.5.2.1–2, and, for the example CLCW, 232.0-B-4 §4.2.1; values from an independent Python model of those clauses, [`models/core_ocf.py`](models/core_ocf.py) |
| `core/layout.json` | Frame field byte ranges with and without secondary header, security, OCF, and FECF (CMN-040, CMN-041, API-075) | Hand-derived from 132.0/232.0/732.0/732.1 §4.1 and 355.0-B-2 §4.1 |
| `spp/field_codes.json` | Error expectations with the code of the field they name: the inputs of four released SPP cases, with `field` added (API-082; M2 plan D3) | Hand-derived from 133.0-B-2 and the field catalogue of [03 §9.1](../docs/03-api-model.md#91-field-codes) |
| `aos/fhec_{encode,correct_1_symbol,correct_2_symbols,miscorrection,uncorrectable}.json` | AOS Frame Header Error Control: encoding, correction of one and two symbol errors, a four-symbol error the code cannot detect and every decoder miscorrects (`miscorrection`), and detected uncorrectable headers, including locator roots in the virtual fill (AOS-013, AOS-014) | Hand-derived from 732.0-B-5 EC1 §4.1.2.6.5 (the same code as 732.0-B-4); 732.0 prints no worked FHEC example; values from an independent Python model written before the Rust codec, [`models/aos_fhec.py`](models/aos_fhec.py); one encoding and one correction worked by hand in [docs/verification/derivations/aos-fhec.md](../docs/verification/derivations/aos-fhec.md) |
| `tm/{minimal,fsh_ocf_fecf,fhp_values,oid_sequence,security_layout,invalid}.json` | TM Transfer Frames: decode (every view field), `decode_with` a security lookup, `decode_unchecked`, `peek_length`, `encode` with the two-phase writer, consecutive OID frames, validation against the master channel, configuration errors, and every decoder and encoder error with its field code (TM-001…TM-011, API-012, API-013, API-070, API-072) | Hand-derived from 132.0-B-3 §4.1, §5, §6.3, and Annex D, and [tm-frame.md](../docs/standards/tm-frame.md); 132.0 prints no example frame, only the first OID octets, which `tm-oid-001` reproduces; values from an independent Python model, [`models/tm_frame.py`](models/tm_frame.py); one decode and one encode worked by hand in [docs/verification/derivations/tm-frame.md](../docs/verification/derivations/tm-frame.md) |

Validate with `cargo xtask vectors validate`, which also runs the independent models in [`models/`](models/README.md) (TV-023). The JSON Schemas in `schema/` are generated from the Rust model (`cargo xtask vectors schema`), so other implementations can validate the same files.

## Field catalogue

`core/fields.json` is not a vector file. It is the field catalogue of [03 §9.1](../docs/03-api-model.md#91-field-codes), generated by `cargo xtask fields --write` so that the C++ and binding runners can name the field codes that error expectations give (`expect.field`). Never edit it: change the table and regenerate; `cargo xtask fields` fails when the file is stale. Its schema is `schema/fields.schema.json`. The format:

```json
{
  "source": "docs/03-api-model.md §9.1, generated by `cargo xtask fields --write`; do not edit",
  "fields": [
    { "code": 1, "name": "spacecraft_id", "kind": "field" },
    { "code": 11, "name": "width", "kind": "parameter" }
  ]
}
```

`fields` lists every entry in code order from 1 with no gap; code 0 is never used ("no field" in the C ABI). `kind` is `field` for a field of a data unit that a Blue Book defines and `parameter` for anything else an error can name. `name` is for display only and is never compared (API-082).

All other directories are created as each milestone adds vectors (see [../docs/08-roadmap.md](../docs/08-roadmap.md)).
