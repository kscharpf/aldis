# Derivation: USLP Transfer Frame vectors

The worked derivation that [07-test-vectors.md](../../07-test-vectors.md) TV-020 asks for a `hand-derived` vector file, for `test-vectors/uslp/*.json` (USLP-001…USLP-016; [M2 plan](../../milestones/m2-frame-codecs.md) WP-5). 732.1-B-3 prints no example frame, only the first octets of the OID sequence (Annex H), and no independent implementation has been run against these vectors yet, so one non-truncated frame, with its FECF, and one truncated frame are worked here by hand. The other cases' values come from the independent Python model the provenance notes name (`test-vectors/models/uslp_frame.py`); this derivation checks that model on two cases. The model computes the FECF as the remainder of the Annex B equation by long division; the register below is the shift-register form of Figure B-1, a different method that gives the same value.

## 1. The rule-000 frame of `uslp-fixed000-001`

Channel: a fixed-length Physical Channel of 16 octets with FECF and no Insert Zone (Table 5-1); SCID `0x155` (the fictional SCID of APP-026), VCID 1, a fixed-length VC with a 1-octet VCF Count for both QoS, no OCF, no SDLS; MAP 0. Frame: sequence-controlled, VCF Count `0x2A`, TFDZ Construction Rule `000`, UPID 0, First Header Pointer 1, TFDZ `AB 08 00`.

### 1.1 Primary Header (732.1 §4.1.2)

Bit 0 is the most significant bit of octet 0. The fields, in the order of §4.1.2.1.2:

| Field | Bits | Value | Binary | Clause |
|---|---|---|---|---|
| Transfer Frame Version Number | 0–3 | 12 | `1100` | §4.1.2.2.2.2 |
| Spacecraft Identifier | 4–19 | `0x155` | `0000 0001 0101 0101` | §4.1.2.2.3 |
| Source-or-Destination Identifier | 20 | 0 (SCID is the source) | `0` | §4.1.2.3.3 |
| Virtual Channel Identifier | 21–26 | 1 | `000001` | §4.1.2.4.1 |
| MAP Identifier | 27–30 | 0 | `0000` | §4.1.2.5.1 |
| End of Frame Primary Header Flag | 31 | 0 (not truncated) | `0` | §4.1.2.6.2 |
| Frame Length | 32–47 | 16 − 1 = 15 | `0000 0000 0000 1111` | §4.1.2.7.2 |
| Bypass/Sequence Control Flag | 48 | 0 (sequence-controlled) | `0` | §4.1.2.8.1.2, Table 4-1 |
| Protocol Control Command Flag | 49 | 0 (user data) | `0` | §4.1.2.8.2.2, Table 4-1 |
| Reserve Spares | 50–51 | 0 | `00` | §4.1.2.9.2 |
| OCF Flag | 52 | 0 | `0` | §4.1.2.10.2 |
| VCF Count Length | 53–55 | 1 octet | `001` | §4.1.2.11, Table 4-2 |
| VCF Count | 56–63 | 42 | `0010 1010` | §4.1.2.12 |

Grouped into octets: `1100 0000` `0001 0101` `0101 0000` `0010 0000` `0000 0000` `0000 1111` `0000 0001` `0010 1010`, that is `C0 15 50 20 00 0F 01 2A`.

### 1.2 Transfer Frame Data Field (§4.1.4)

The VC is fixed-length, so the TFDZ is fixed-length (§4.1.4.2.2.1.1) and rule `000` applies (§4.1.4.2.2.2.1, Table 4-3), which carries the 16-bit First Header Pointer (§4.1.4.2.4.1, §4.1.4.2.4.3): the TFDF Header is 3 octets.

| Field | Bits | Value | Binary | Clause |
|---|---|---|---|---|
| TFDZ Construction Rule | 0–2 | `000` | `000` | §4.1.4.2.2.1.2 |
| USLP Protocol Identifier | 3–7 | 0, the Space Packet or Encapsulation Packet row of `specs/sana/uslp_protocol_id.tsv` | `00000` | §4.1.4.2.3 |
| First Header Pointer | 8–23 | 1 | `0000 0000 0000 0001` | §4.1.4.2.4.2–3 |

TFDF Header `00 00 01`. The TFDZ is the rest of the frame before the FECF: 16 − 8 − 3 − 2 = 3 octets (§4.1.4.1.2), `AB 08 00`; the pointer 1 is below 3, so it names an octet of the zone (USLP-006). The frame before its FECF is `C0 15 50 20 00 0F 01 2A 00 00 01 AB 08 00`.

### 1.3 FECF (732.1 §4.1.6, Annex B)

G(X) = X¹⁶ + X¹² + X⁵ + 1 (B1.1.3), so the feedback mask is `0x1021`; the term X⁽ⁿ⁻¹⁶⁾·L(X) presets the register to all ones (B1.1.3 note 1). Each message bit, first transmitted first, is added to the register's most significant bit: if the sum is 1, the register is shifted left and `0x1021` is added; otherwise it is only shifted (Figure B-1).

The first octet, `0xC0` = `1100 0000`, bit by bit from `FFFF`:

| Bit | In | Register MSB | Feedback | Register after |
|---|---|---|---|---|
| 0 | 1 | 1 | 0 | `FFFE` |
| 1 | 1 | 1 | 0 | `FFFC` |
| 2 | 0 | 1 | 1 | `FFF8` ⊕ `1021` = `EFD9` |
| 3 | 0 | 1 | 1 | `DFB2` ⊕ `1021` = `CF93` |
| 4 | 0 | 1 | 1 | `9F26` ⊕ `1021` = `8F07` |
| 5 | 0 | 1 | 1 | `1E0E` ⊕ `1021` = `0E2F` |
| 6 | 0 | 0 | 0 | `1C5E` |
| 7 | 0 | 0 | 0 | `38BC` |

The other octets the same way, eight steps each:

| Octet | Register before | Register after |
|---|---|---|
| `C0` | `FFFF` | `38BC` |
| `15` | `38BC` | `49CF` |
| `50` | `49CF` | `4C18` |
| `20` | `4C18` | `B52A` |
| `00` | `B52A` | `DD7E` |
| `0F` | `DD7E` | `953F` |
| `01` | `953F` | `FC3D` |
| `2A` | `FC3D` | `96BB` |
| `00` | `96BB` | `587F` |
| `00` | `587F` | `A4FD` |
| `01` | `A4FD` | `184F` |
| `AB` | `184F` | `D8B8` |
| `08` | `D8B8` | `737D` |
| `00` | `737D` | `33F4` |

FECF = `33F4`, sent most significant bit first (B1.1.3). The frame is `C0 15 50 20 00 0F 01 2A 00 00 01 AB 08 00 33 F4`, the `output_hex` of `uslp-fixed000-001` and the input of `uslp-fixed000-002`. The same register run over all sixteen octets ends at `0000`, the zero syndrome of B1.2.1. The decoded view of `uslp-fixed000-002` gives the fields of §1.1 and §1.2 back, with `frame_len` 16 (GEN-053), `pointer_kind` `packet_starts_at`, and `fecf_hex` `33F4`.

## 2. The truncated frame of `uslp-trunc-001` (732.1 Annex D)

Channel: a variable-length Physical Channel; SCID `0x155`, VCID 5, a variable-length VC whose Truncated Transfer Frame Length is 12 octets (Table 5-3, Annex D1.2.1, D1.3.2), no SDLS; MAP 2. Frame: TFDZ Construction Rule `111`, UPID 5 (the Mission-specific Information-1 row of the SANA snapshot), TFDZ `31 32 33 34 35 36 37`.

### 2.1 Truncated Primary Header (Annex D1.3.1, Figure D-2)

| Field | Bits | Value | Binary | Clause |
|---|---|---|---|---|
| Transfer Frame Version Number | 0–3 | 12 | `1100` | §4.1.2.2.2.2 |
| Spacecraft Identifier | 4–19 | `0x155` | `0000 0001 0101 0101` | §4.1.2.2.3 |
| Source-or-Destination Identifier | 20 | 0 | `0` | §4.1.2.3.3 |
| Virtual Channel Identifier | 21–26 | 5 | `000101` | §4.1.2.4.1 |
| MAP Identifier | 27–30 | 2 | `0010` | §4.1.2.5.1 |
| End of Frame Primary Header Flag | 31 | 1 (truncated) | `1` | §4.1.2.6.2 b) |

Grouped into octets: `1100 0000` `0001 0101` `0101 0000` `1010 0101`, that is `C0 15 50 A5`. The header has no Frame Length: the length is the VC's managed parameter (D1.3.2), and the frame has no Insert Zone, OCF, or FECF (D1.2.2 note 1).

### 2.2 Transfer Frame Data Field (Annex D1.4)

The TFDF Header is one octet, without the pointer (D1.4.1.2): rule `111` and UPID 5, `111 00101` = `E5`. The rule is one of the variable-length rules, as a truncated frame occurs only on a variable-length VC (D1.2.1, USLP-016); the notes to D1.4.1.2 suggest `111` and Mission-specific Information-1, which this case follows and which the decoder exposes without checking (OI-35 item 1). The TFDZ is 12 − 4 − 1 = 7 octets (D1.3.2.2, D1.4.2.4).

The frame is `C0 15 50 A5 E5 31 32 33 34 35 36 37`, the `output_hex` of `uslp-trunc-001` and the input of `uslp-trunc-002`, whose view gives `truncated` true, `frame_len` 12, and `consumed` 12.
