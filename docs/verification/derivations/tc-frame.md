# Derivation: TC Transfer Frame and CLCW vectors

The worked derivation that [07-test-vectors.md](../../07-test-vectors.md) TV-020 asks for a `hand-derived` vector file, for `test-vectors/tc/*.json` (TC-001…TC-009, TC-020…TC-023, TC-030, TC-031; [M2 plan](../../milestones/m2-frame-codecs.md) WP-3). 232.0-B-4 prints no example frame or CLCW, and no independent implementation has been run against these vectors yet (D15), so one frame, with its FECF, and one CLCW are worked here by hand. The other cases' values come from the independent Python model the provenance notes name (`test-vectors/models/tc_frame.py`); this derivation checks that model on two cases.

## 1. The BC Unlock frame of `tc-bc-unlock-enc-001`

Channel: SCID `0x155` (the fictional SCID of APP-026), VCID 1 without Segment Header, FECF present. Frame: Type-BC, N(S) 0, the Unlock Control Command.

### 1.1 Primary Header (232.0 §4.1.2)

Bit 0 is the most significant bit of octet 0 (§1.6.2). The fields, in order:

| Field | Bits | Value | Binary | Clause |
|---|---|---|---|---|
| Transfer Frame Version Number | 0–1 | 0 | `00` | §4.1.2.2.2 |
| Bypass Flag | 2 | 1 (Type-B) | `1` | §4.1.2.3.1, Table 4-1 (`11` = Type-BC) |
| Control Command Flag | 3 | 1 (Type-C) | `1` | §4.1.2.3.2 |
| Reserved Spare | 4–5 | 0 | `00` | §4.1.2.4.2 |
| Spacecraft Identifier | 6–15 | `0x155` | `01 0101 0101` | §4.1.2.5 |
| Virtual Channel Identifier | 16–21 | 1 | `000001` | §4.1.2.6 |
| Frame Length | 22–31 | 8 − 1 = 7 | `00 0000 0111` | §4.1.2.7.2: total octets minus one; the frame is 5 + 1 + 2 = 8 octets |
| Frame Sequence Number | 32–39 | 0 | `0000 0000` | §4.1.2.8 |

Grouped into octets: `0011 0001` `0101 0101` `0000 0100` `0000 0111` `0000 0000`, that is `31 55 04 07 00`.

### 1.2 Data field (232.0 §4.1.3.3.2)

Unlock is one octet of zeros: `00`. A Type-C frame carries no Segment Header (§4.1.3.2.2.1.3) and no SDLS fields (§6.3.4.1).

### 1.3 FECF (232.0 §4.1.4.2)

G(X) = X¹⁶ + X¹² + X⁵ + 1, so the feedback mask is `0x1021`; the register starts at all ones (note 1 to §4.1.4.2.2), and each message bit, first transmitted first, is added to the register's most significant bit: if the sum is 1, the register is shifted left and `0x1021` is added; otherwise it is only shifted.

The first octet, `0x31` = `0011 0001`, bit by bit from `FFFF`:

| Bit | In | Register MSB | Feedback | Register after |
|---|---|---|---|---|
| 0 | 0 | 1 | 1 | `FFFE` ⊕ `1021` = `EFDF` |
| 1 | 0 | 1 | 1 | `DFBE` ⊕ `1021` = `CF9F` |
| 2 | 1 | 1 | 0 | `9F3E` |
| 3 | 1 | 1 | 0 | `3E7C` |
| 4 | 0 | 0 | 0 | `7CF8` |
| 5 | 0 | 0 | 0 | `F9F0` |
| 6 | 0 | 1 | 1 | `F3E0` ⊕ `1021` = `E3C1` |
| 7 | 1 | 1 | 0 | `C782` |

The other octets the same way, eight steps each:

| Octet | Register before | Register after |
|---|---|---|
| `31` | `FFFF` | `C782` |
| `55` | `C782` | `21FB` |
| `04` | `21FB` | `8FC7` |
| `07` | `8FC7` | `D780` |
| `00` | `D780` | `3B9A` |
| `00` | `3B9A` | `1D38` |

FECF = `1D38`, sent most significant bit first (§4.1.4.2.2). The frame is `31 55 04 07 00 00 1D 38`, the `output_hex` of `tc-bc-unlock-enc-001` and the input of `tc-bc-unlock-dec-001`. The same register run over all eight octets ends at `0000`, the zero syndrome of §4.1.4.3. As a check of the method, the same procedure over the ASCII octets of `123456789` gives `29B1`, the check value of CMN-002.

## 2. The CLCW of `tc-clcw-enc-001` (232.0 §4.2.1)

COP-1, VCID 5, Retransmit set, FARM-B Counter 2, Report Value 42, everything else zero.

| Field | Bits | Value | Binary | Clause |
|---|---|---|---|---|
| Control Word Type | 0 | 0 | `0` | §4.2.1.2.2 |
| CLCW Version Number | 1–2 | 0 | `00` | §4.2.1.3.2 |
| Status Field | 3–5 | 0 | `000` | §4.2.1.4 |
| COP in Effect | 6–7 | 1 (COP-1) | `01` | §4.2.1.5.2 |
| Virtual Channel Identification | 8–13 | 5 | `000101` | §4.2.1.6 |
| Reserved Spare | 14–15 | 0 | `00` | §4.2.1.7.2 |
| No RF Available, No Bit Lock, Lockout, Wait, Retransmit | 16–20 | 0, 0, 0, 0, 1 | `00001` | §4.2.1.8 |
| FARM-B Counter | 21–22 | 2 | `10` | §4.2.1.9 |
| Reserved Spare | 23 | 0 | `0` | §4.2.1.10.2 |
| Report Value | 24–31 | 42 | `0010 1010` | §4.2.1.11 |

Grouped into octets: `0000 0001` `0001 0100` `0000 1100` `0010 1010`, that is `01 14 0C 2A`, the `output_hex` of `tc-clcw-enc-001`, the input of `tc-clcw-dec-001`, and the CLCW that `core/ocf.json` assembles for `ocf-classify-001` from the same figure.
