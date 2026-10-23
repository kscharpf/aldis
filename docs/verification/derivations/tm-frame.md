# Derivation: TM Transfer Frame vectors

The worked derivation that [07-test-vectors.md](../../07-test-vectors.md) TV-020 asks for a `hand-derived` vector file, for `test-vectors/tm/*.json` (TM-001…TM-011; [M2 plan](../../milestones/m2-frame-codecs.md) WP-2). 132.0-B-3 prints no example frame, only the first OID octets of Annex D, and no independent implementation has been run against these vectors yet (D15), so one frame is taken apart here by hand, field by field, and one frame is assembled. The other cases' values come from the independent Python model [`test-vectors/models/tm_frame.py`](../../../test-vectors/models/tm_frame.py) that the provenance notes name; this derivation checks that model on two cases.

## 1. The format

From 132.0-B-3 §4.1.2 (figures 4-2 and 4-3) and §4.1.3 (figure 4-4). Bit 0 is the most significant bit of octet 0 (§1.6).

| Bits | Field | Width | Clause |
|---|---|---|---|
| 0–1 | Transfer Frame Version Number, `00` | 2 | §4.1.2.2.2 |
| 2–11 | Spacecraft Identifier | 10 | §4.1.2.2.3 |
| 12–14 | Virtual Channel Identifier | 3 | §4.1.2.3 |
| 15 | OCF Flag | 1 | §4.1.2.4 |
| 16–23 | Master Channel Frame Count | 8 | §4.1.2.5 |
| 24–31 | Virtual Channel Frame Count | 8 | §4.1.2.6 |
| 32 | Secondary Header Flag | 1 | §4.1.2.7.2 |
| 33 | Synchronization Flag | 1 | §4.1.2.7.3 |
| 34 | Packet Order Flag | 1 | §4.1.2.7.4 |
| 35–36 | Segment Length Identifier, `11` with Synchronization Flag `0` | 2 | §4.1.2.7.5 |
| 37–47 | First Header Pointer | 11 | §4.1.2.7.6 |

The Secondary Header, if the flag is set, starts with one octet: its version `00` in bits 0–1 and its total length minus one in bits 2–7 (§4.1.3.2), then 1 to 63 data octets (§4.1.3.1.3). The data field takes what the fixed frame length leaves after the headers, the Security Header and Trailer, the OCF, and the FECF (§4.1.4.2, §6.3.5.2). The FECF is the CRC of §4.1.6.2: generator X¹⁶ + X¹² + X⁵ + 1, register preset to ones, the frame octets fed most significant bit first ([common-link-features.md](../../standards/common-link-features.md) §1).

## 2. Decoding `tm-fof-dec-005`

Configuration: 12-octet frames with the FECF. Input `00 12 00 00 98 00 01 99 01 02 34 29`.

| Octets | Bits | Field | Value |
|---|---|---|---|
| `00` | `00` `000000` | version; SCID bits 2–7 | 0; SCID high part 0 |
| `12` | `0001` `001` `0` | SCID bits 8–11; VCID; OCF Flag | SCID = `0000000001` = 1; VCID 1; no OCF |
| `00` `00` | | MC and VC Frame Counts | 0, 0 |
| `98` | `1` `0` `0` `11` `000` | FSH Flag; Sync; POF; SLID; FHP bits 37–39 | FSH present; packets; POF 0; SLID `11` |
| `00` | `00000000` | FHP bits 40–47 | FHP = 0 |
| `01` | `00` `000001` | FSH version; FSH length − 1 | version `00`; FSH of 2 octets |
| `99` | | FSH data field | `99` |

The layout is then 6 (Primary Header) + 2 (FSH) + 0 (no SDLS on a channel decoded with `decode`) + 0 (no OCF) + 2 (FECF) = 10 octets of fixed fields, leaving a data field of 12 − 10 = 2 octets: `01 02`. The Synchronization Flag is `0`, so the First Header Pointer is classified: 0 is neither `0x7FE` nor `0x7FF`, and 0 < 2, so a packet starts at data octet 0 (TM-008). The Packet Order Flag is `0` and the Segment Length Identifier `11`, so a strict decode raises nothing (TM-007).

The FECF is the last two octets, `34 29`. The CRC register, preset to `FFFF`, after each of the ten covered octets (the register shifted left eight times per octet, `1021` added whenever a one leaves bit 15):

| Octet | `00` | `12` | `00` | `00` | `98` | `00` | `01` | `99` | `01` | `02` |
|---|---|---|---|---|---|---|---|---|---|---|
| Register | `E1F0` | `2F7C` | `A98D` | `A9C3` | `E572` | `DF8B` | `A1B3` | `045B` | `0BA5` | `3429` |

The register ends at `3429`, equal to the FECF, so the check passes (TM-005), and the view reports `fecf` = 0x3429 = 13353. These are the values the vector file expects.

## 3. Encoding `tm-fof-enc-001`

Configuration: 40-octet frames with the FECF; SCID 677 (`0x2A5`); VC 1 carries packets, with a 4-octet VC_FSH and a VC_OCF. Fields: MC count 3, VC count 4, a packet starting at data octet 2; FSH data `11 22 33`; OCF `01 14 0C 2A` (the CLCW of `core/ocf.json`).

- The writer computes the OCF Flag (1) and the Secondary Header Flag (1) from the configuration (TM-009, API-021). The Synchronization Flag of a packet VC is `0`, so the Packet Order Flag is `0`, the Segment Length Identifier `11`, and the First Header Pointer 2.
- Octets 0–1: `00` ‖ `1010100101` ‖ `001` ‖ `1` = `0010 1010 0101 0011` = `2A 53`. Octets 2–3: `03 04`. Octets 4–5: `1` `0` `0` `11` `000 00000010` = `98 02`.
- Octet 6, the FSH Identification Field: version `00`, length 4 − 1 = 3: `03`; then `11 22 33`.
- The data field is 40 − 6 − 4 − 4 − 2 = 24 octets, `40 41 … 57`, then the OCF `01 14 0C 2A`.
- The FECF over the first 38 octets is `D3 76`.

The frame is `2A53 0304 9802 0311 2233 4041 … 5657 0114 0C2A D376`, the expected output. The two-phase writer produces the same octets (API-020).

## 4. What the model adds

`test-vectors/models/tm_frame.py` computes every expected value of the six files with a different method from the Rust codec: frames are assembled and taken apart as strings of bits from the widths above, the FECF is the remainder of a division of Python integers (§4.1.6.2.2), and the OID data is the recurrence of Annex D on a list of bits. Its OID generator reproduces the first 20 octets of Annex D (`FF FF FF FF 6D B6 D8 61 45 1F 11 F1 97 16 72 3C BE 7E 00 B1`), which `tm-oid-001` shows in its first frame, and its CRC gives `29B1` for `"123456789"` (CMN-002). `python3 -I test-vectors/models/tm_frame.py --check test-vectors/tm/*.json` recomputes every case (TV-023).
