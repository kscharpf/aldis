# Derivation: AOS Transfer Frame vectors

The worked derivation that [07-test-vectors.md](../../07-test-vectors.md) TV-020 asks for a `hand-derived` vector file, for the AOS frame files of `test-vectors/aos/` (`minimal`, `fhec_frame`, `insert_zone`, `security_layout`, `mpdu_pointers`, `bpdu_pointers`, `oid`, `issue4_compat`, `scid_10bit`, `config`, `invalid`; AOS-001…AOS-019, GEN-062; [M2 plan](../../milestones/m2-frame-codecs.md) WP-4). 732.0-B-5 EC1 prints no example frame, only the first OID octets of Annex D, and no independent implementation has been run against these vectors yet (D15), so one frame is taken apart here by hand, field by field, one is assembled, and the order of the FHEC and the FECF is worked on a miscorrected header. The other cases' values come from the independent Python model [`test-vectors/models/aos_frame.py`](../../../test-vectors/models/aos_frame.py) that the provenance notes name; this derivation checks that model on three cases. The FHEC itself is derived in [aos-fhec.md](aos-fhec.md).

## 1. The format

From 732.0-B-5 EC1 §4.1.2 (figure 4-2) and §4.1.4.2–3 (figures 4-3 and 4-4). Bit 0 is the most significant bit of octet 0 (§1.6).

| Bits | Field | Width | Clause |
|---|---|---|---|
| 0–1 | Transfer Frame Version Number, `01` | 2 | §4.1.2.2.2 |
| 2–9 | Spacecraft Identifier, 8 least significant bits | 8 | §4.1.2.2.3.1 |
| 10–15 | Virtual Channel Identifier; 63 for OID frames | 6 | §4.1.2.3, §4.1.4.1.5.1 |
| 16–39 | Virtual Channel Frame Count | 24 | §4.1.2.4 |
| 40 | Replay Flag | 1 | §4.1.2.5.2 |
| 41 | VC Frame Count Cycle Use Flag | 1 | §4.1.2.5.3 |
| 42–43 | Spacecraft Identifier, 2 most significant bits (Issue 5); a reserved spare `00` in Issue 4 and earlier (732.0-B-4 §4.1.2.5.4) | 2 | §4.1.2.5.4 |
| 44–47 | VC Frame Count Cycle; `0000` when unused | 4 | §4.1.2.5.5 |
| 48–63 | Frame Header Error Control, if the physical channel has one | 16 | §4.1.2.6 |

The Insert Zone, if configured, follows the header (§4.1.3), then the Security Header (§6.3.4), the data field, the Security Trailer, the OCF if the virtual channel has one (§4.1.5.2), and the FECF if the physical channel has one (§4.1.6). The data field takes what the fixed frame length leaves (§4.1.4.1.2, §6.3.5.2 b). An M_PDU starts with a 16-bit First Header Pointer in Issue 5 (§4.1.4.2.1.4), a 5-bit spare and an 11-bit pointer in Issue 4 (732.0-B-4 §4.1.4.2.1.4); a B_PDU with a 2-bit spare and a 14-bit Bitstream Data Pointer (§4.1.4.3.1.4). The FECF is the CRC of §4.1.6.2: generator X¹⁶ + X¹² + X⁵ + 1, register preset to ones, the octets fed most significant bit first ([common-link-features.md](../../standards/common-link-features.md) §1).

## 2. Decoding `aos-min-dec-006`

Configuration: 18-octet frames, Issue 5, no FHEC or Insert Zone, with the FECF; SCID 85 (`0x55`), VC 5 carries M_PDUs. Input `55 45 00 00 01 00 FF FF 20 21 22 23 24 25 26 27 8D 48`.

| Octets | Bits | Field | Value |
|---|---|---|---|
| `55` | `01` `010101` | version; SCID bits 2–7 | version `01`; SCID LSB high part `010101` |
| `45` | `01` `000101` | SCID bits 8–9; VCID | SCID LSBs `01010101` = `0x55`; VCID 5 |
| `00 00 01` | | VC Frame Count | 1 |
| `00` | `0` `0` `00` `0000` | Replay; Use Flag; bits 42–43; cycle | realtime; cycle unused; SCID MSBs `00`, so SCID = `0x055` = 85; cycle `0000` |

The layout is 6 (Primary Header) + 0 (Insert Zone) + 0 (no SDLS on VC 5) + 0 (no OCF) + 2 (FECF) = 8 octets of fixed fields, leaving a data field of 18 − 8 = 10 octets: `FF FF 20 … 27`. VC 5 carries M_PDUs, so its first two octets are the M_PDU Header: First Header Pointer `0xFFFF`, all ones, "no Packet starts" (§4.1.4.2.2.4); the Packet Zone is the 8 octets `20 … 27`. The Use Flag is `0` and the cycle `0000`, so a strict decode raises nothing (AOS-012).

The FECF is the last two octets, `8D 48`. The CRC register, preset to `FFFF`, after each of the sixteen covered octets (the register shifted left eight times per octet, `1021` added whenever a one leaves bit 15):

| Octet | `55` | `45` | `00` | `00` | `01` | `00` | `FF` | `FF` |
|---|---|---|---|---|---|---|---|---|
| Register | `EBA0` | `F424` | `8B9B` | `BBE3` | `E591` | `3C8B` | `622F` | `7D14` |

| Octet | `20` | `21` | `22` | `23` | `24` | `25` | `26` | `27` |
|---|---|---|---|---|---|---|---|---|
| Register | `9F58` | `1E15` | `E2DF` | `166D` | `7B11` | `AA3B` | `6B04` | `8D48` |

The register ends at `8D48`, equal to the FECF, so the check passes, and the view reports `fecf` = 0x8D48 = 36168. These are the values the vector file expects.

## 3. Encoding `aos-i4-enc-001`

Configuration: 14-octet frames, `Issue4Compatible`, no FHEC, Insert Zone, or FECF; SCID 165 (`0xA5`), VC 5 carries M_PDUs. Fields: VC Frame Count `0x010203`, Replay Flag 1, cycle 12, a packet starting at Packet Zone octet 5; Packet Zone `70 71 72 73 74 75`.

- Octets 0–1: `01` ‖ `10100101` ‖ `000101` = `0110 1001 0100 0101` = `69 45`. Issue 4 has an 8-bit SCID, so the whole SCID is in bits 2–9 (AOS-002).
- Octets 2–4: `01 02 03`.
- Octet 5: Replay `1`, Use Flag `1` (a cycle is given), bits 42–43 the reserved spare written `00` (732.0-B-4 §4.1.2.5.4; GEN-052), cycle `1100`: `1100 1100` = `CC`.
- The data field is 14 − 6 = 8 octets. The M_PDU Header in Issue 4 is the spare `00000` and the 11-bit pointer 5: `00000` ‖ `00000000101` = `00 05` (732.0-B-4 §4.1.4.2.1.4; AOS-003). Then the Packet Zone, 6 octets.

The frame is `69 45 01 02 03 CC 00 05 70 71 72 73 74 75`, the expected output. The two-phase writer produces the same octets (API-020).

## 4. The FHEC before the FECF: `aos-fhecf-dec-006`

Configuration: 24-octet frames, Issue 5, with FHEC and FECF; VCs (SCID 170, VC 5) and (SCID 746, VC 37), both `VCA_SDU`s. The sender's header is `6A 85 12 34 56 00 12 52` (`aos-fhec-enc-003` of `fhec_encode.json`: SCID `0xAA`, VC 5, count `0x123456`), the data field fourteen octets `33`, and the FECF over those 22 octets is `37 63`.

The frame is received with the four symbol errors of `aos-fhec-mis-001`: octet 0 `6A` → `7A`, octet 1 `85` → `A5`, octet 5 `00` → `30`, octet 6 `12` → `52`. The receiving end corrects the header first (§4.3.7.2): the received header `7A A5 12 34 56 30 52 52` lies within two symbols of the codeword `7A A5 12 34 56 E0 53 52`, so the FHEC decoder returns that codeword with two corrections ([aos-fhec.md](aos-fhec.md); 732.0 §4.1.2.6.5 note 3; OI-35 item 5). Its fields would be SCID `0x2EA` = 746 (LSBs `11101010` from bits 2–9, MSBs `10` from bits 42–43), VCID 37, Replay and Use Flags 1: a valid channel of the configuration.

Then the FECF is recomputed over the frame with the corrected header (§4.3.7.3; AOS-014): the CRC of `7AA5123456E05352` followed by the fourteen `33` octets is `179C`, not the received `3763`, so the decoder returns `CrcMismatch`. (Over the header as received, `7AA5123456305252`, the CRC would be `29B5`; over the sender's header, `3763`.) The companion case `aos-fhecf-dec-007` sends the same header without an FECF: the miscorrected frame then decodes as SCID 746, VC 37, with `corrected_symbols` 2, as AOS-014 says no decoder can prevent.

## 5. What the model adds

`test-vectors/models/aos_frame.py` computes every expected value of the eleven files with a method different from the Rust codec: frames are assembled and taken apart as strings of bits from the widths above, the FECF is the remainder of a division of Python integers (§4.1.6.2.2) over the corrected header and the rest of the frame, the OID data is the recurrence of Annex D on a list of bits, and the FHEC is the table model of [`aos_fhec.py`](../../../test-vectors/models/aos_fhec.py). Its OID generator reproduces the first octets of Annex D (`FF FF FF FF 6D B6 D8 61 …`), which `aos-oid-enc-001` shows in its first frame, and its second frame continues the sequence with `45 1F 11 F1 97 16 72 3C` (CMN-021). `python3 -I test-vectors/models/aos_frame.py --check` on the eleven files recomputes every case (TV-023).
