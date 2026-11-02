# Derivation: receiving-side link engine vectors

The worked derivation that [07-test-vectors.md](../../07-test-vectors.md) TV-020 asks for a `hand-derived` vector file, for `test-vectors/link/recv/*.json` (LNK-050…054, LNK-077; model `test-vectors/models/link_recv.py`). Bit 0 is the most significant bit of octet 0. Three frames are decoded by hand and the expected outputs derived from the clauses of 04.

## 1. TM OID frame, `link-recv-tm-oid-frame`, step 2 (132.0 §4.1.2, §4.1.3, §4.1.4.6; LNK-051, LNK-052)

Configuration: 48-octet frames with FECF, SCID 291 (`0x123`), VC 7 a packets VC. Input: `123F 1F 64 9FFE | 01 AA | FFFFFFFF6DB6…B262…(34 octets) | 01040805 | BF9E`.

| Octets | Field | Value |
|---|---|---|
| `123F` = `0001 0010 0011 1111` | TFVN `00`, SCID `0100100011` = 291, VCID `111` = 7, OCF flag `1` | §4.1.2.2–4 |
| `1F` | MC Frame Count | 31 |
| `64` | VC Frame Count | 100 |
| `9FFE` = `1 0 0 11 11111111110` | Secondary Header flag 1, sync 0, order 0, segment length `11`, FHP `0x7FE` | an OID frame (§4.1.2.7.6.5) |
| `01 AA` | Secondary Header: ID octet `00 000001` (length 2 − 1), one data octet | §4.1.3.2.3.2 |
| next 34 octets | data field = 48 − 6 − 2 − 4 − 2; the OID sequence from its start, `FFFFFFFF6DB6…` (the first OID frame of the generator) | §4.1.4.6.2 |
| `01040805` | OCF, bit 0 = 0: Type-1 | §4.1.5.4 |
| `BF9E` | FECF over the preceding 46 octets | §4.1.6 |

Expected: the previous frame was MC 30/VC 3 of VC 0 and the VC 7 expectation is new, so no gap (LNK-051). The frame is an OID frame: FSH SDU `01AA` and the OCF delivered, no data field, no FrameDiscarded (LNK-052).

## 2. AOS B_PDU, `link-recv-aos-bitstream`, step 2 (732.0 §4.1.2, §4.1.4.3, §4.3.3.2; LNK-077)

Configuration: 48-octet frames with FECF, Insert Zone 4 octets, VC 3 bitstream. Input `6943 000002 20 DEADBEEF 0013 FF…FF 94 4E`.

`6943` = `01 01001001 000011`: TFVN `01`, SCID LSBs `0xA5`, VCID 3 (§4.1.2.2–3); with SCID MSBs `10` in octet 5 (`20` = `0010 0000`: replay 0, cycle-use 0, SCID MSB `10`, cycle 0) SCID = `10 10100101` = 677. `000002` is VC Frame Count 2. The Insert Zone is `DEADBEEF`. The B_PDU header `0013`: spare `00`, Bitstream Data Pointer 19 = the last valid bit (§4.1.4.3.3.2), so 20 bits are valid (not all ones: some idle, §4.1.4.3.3.3). The zone is `FF` octets; the valid bits are 20: `FF FF F` and the 4 unused bits of the third octet zero: `FFFFF0`. Expected: Insert SDU, then DataField with `bit_count` 20 and `FFFFF0`.

## 3. USLP frame, `link-recv-uslp-vcf-gap`, step 5 (732.1 §4.1.2, §4.3.8.3; LNK-051)

Input `CC0DE020 002F 02 00 0E | A1A2A3 | TFDF… | 7C0D`, 48 octets.

`CC0DE020`: TFVN `1100`, SCID `0xC0DE`, source/destination 0, VCID `000001` = 1, MAP ID 0, EOFPH 0. `002F`: Frame Length 47 = 48 − 1. `02`: bypass 0 (sequence-controlled), command flag 0, spares 0, OCF flag 0, VCF Count Length `010` = 2 octets. `000E`: VCF Count 14. Insert Zone `A1A2A3`.

The sequence-controlled counts seen before were 10 and 11, so the expectation is 12; 14 − 12 = 2 frames missing (LNK-051). Expedited counts (200, 201) are a separate counter and unaffected. Expected: `FrameGap` (VC 1, missing 2), Insert SDU, DataField with `gap` true.
