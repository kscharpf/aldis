# Derivation: sending-side link engine scenarios

The worked derivation that [07-test-vectors.md](../../07-test-vectors.md) TV-020 asks for a `hand-derived` vector file, for `test-vectors/link/send/*.json` (LNK-011…LNK-015, LNK-020, LNK-021, LNK-030…LNK-036, LNK-040…LNK-042; [M4 plan](../../milestones/m4-link-engines.md) WP-2a). The Blue Books print no example of a generated frame, only the first octets of the OID pattern (132.0-B-3 Annex D, 732.1-B-3 Annex H), so four cases are worked from the clauses with no program. The expected octets are the ones in the files; the model `test-vectors/models/link_send.py` computes them by another route (it assembles bit strings and runs the engines as steps).

## 1. A Space Packet Idle Packet that spills (`link-send-pg-tm-lnk-013-spp-idle-spill`)

Configuration: TM data field of 24 octets, release deadline 1 000 µs, Space Packet idle packets, idle pattern `00`.

- At t = 0 a Space Packet of 21 octets (APID 11: header `00 0B C0 00 00 0E`, 15 data octets `71 72 … 7F`) is written to the field, which is started. At t = 1 000 µs the deadline has passed since it started; `Timeout` marks the field releasable (LNK-012).
- `Release` completes the field: 24 − 21 = 3 octets remain, below the 7 of the shortest Space Packet Idle Packet (132.0 §4.2.2.5 note). The idle packet is begun with its first 3 octets `07 FF C0` (APID `0x7FF`, Secondary Header Flag 0 as 133.0 §4.1.3.3.3.4 requires, Sequence Flags `11`); its last 7 − 3 = 4 octets `00 00 00 00` (the Packet Data Length `00 00` of a 7-octet packet and one data octet `00`) are the first octets of the next field.
- The FHP of the released field is 0, the start of the first packet. The next field is started at the `Release` (1 000 µs) with the 4 residue octets, and its deadline is 2 000 µs, which the step asserts. A packet submitted at 1 500 µs follows the residue at offset 4, so the FHP of that field is 4 and not 0 (the residue is not a packet start, 132.0 §4.1.2.7.6.3 note 3).

## 2. A TM OID frame (`link-send-mux-tm-lnk-030-oid-frames`, first frame)

Configuration: SCID 165 (`0x0A5`), 24-octet frames with the FECF, the dedicated VC 7 (132.0 §4.1.4.6.3), no OCF and no Secondary Header.

- Octets 0–1 (figure 4-2): TFVN `00`, SCID `00 1010 0101`, VCID `111`, OCF Flag `0`: `0000 1010 0101 1110` = `0A 5E`.
- Octets 2–3: MC and VC Frame Counts, both 0 for the first frame (LNK-032): `00 00`.
- Octets 4–5 (figure 4-3): Secondary Header Flag `0`, Synchronization Flag `0`, Packet Order Flag `0`, Segment Length Identifier `11`, FHP `111 1111 1110` (§4.1.4.6.1): `0001 1111 1111 1110` = `1F FE`.
- Data field: 24 − 6 − 2 = 16 octets of the OID pattern from the start: `FF FF FF FF 6D B6 D8 61 45 1F 11 F1 97 16 72 3C` (Annex D prints these and four more, `BE 7E 00 B1`, which the second frame starts with, `0A 5E 01 01 1F FE BE 7E 00 B1 …`: the generator is not restarted).
- FECF `00 00`: not yet computed (API-074).

The frame is `0A5E 0000 1FFE FFFFFFFF 6DB6D861 451F11F1 9716723C 0000`.

## 3. An AOS OID frame with a 10-bit SCID (`link-send-mux-aos-lnk-030-oid-frames`, first frame)

Configuration: SCID 421 (`0x1A5`: LSB `0xA5`, MSB `01`), 28-octet frames with the FECF, no FHEC, no Insert Zone.

- Bits 0–1: TFVN `01`; bits 2–9: the 8 LSBs of the SCID `1010 0101`; bits 10–15: VCID `111111` (§4.1.4.1.5.1): `0110 1001 0111 1111` = `69 7F`.
- Bits 16–39: VC Frame Count `00 00 00` (counted like any VC, LNK-032).
- Bits 40–47: Replay Flag `0`, VCF Count Cycle Use Flag `0`, the 2 MSBs of the SCID `01` (bits 42–43, §4.1.2.5.4), Cycle `0000`: `0001 0000` = `10`.
- The header is 6 octets without the FHEC; the data field is 28 − 6 − 2 = 20 octets of the OID pattern, `FF FF FF FF 6D B6 D8 61 45 1F 11 F1 97 16 72 3C BE 7E 00 B1`, then the FECF `00 00`.

The frame is `697F 000000 10 FFFFFFFF6DB6D861451F11F19716723CBE7E00B1 0000`.

## 4. A USLP OID frame (`link-send-mux-uslp-lnk-030-oid-frames`, first frame)

Configuration: SCID `0x1234`, 32-octet frames with the FECF, no Insert Zone, no VCF Count on VCID 63.

- Bits 0–3 TFVN `1100`; bits 4–19 SCID `0001 0010 0011 0100`; bit 20 Source-or-Destination `0`; bits 21–26 VCID `111111`; bits 27–30 MAP ID `0000` (§4.1.4.1.8); bit 31 End of Frame Primary Header Flag `0`: `1100 0001 | 0010 0011 | 0100 0111 | 1110 0000` = `C1 23 47 E0`.
- Bits 32–47: Frame Length = 32 − 1 = 31: `00 1F`.
- Bits 48–55: Bypass/Sequence Control Flag `0` (the configured QoS of the OID frame, which the Blue Book does not state), Protocol Control Command Flag `0`, spares `00`, OCF Flag `0`, VCF Count Length `000`: `00`. No VCF Count follows.
- The header is 7 octets; the TFDF is 32 − 7 − 2 = 23 octets: the TFDF Header `0011 1111` = `3F` (rule `001`, UPID `11111` Only Idle Data, 732.1-B-3 §4.1.4.1.9 and the SANA registry), the pointer to the last octet of the TFDZ, 20 − 1 = 19 = `00 13`, and the TFDZ of 20 octets of the pattern `FF FF FF FF 6D B6 D8 61 45 1F 11 F1 97 16 72 3C BE 7E 00 B1`, then the FECF `00 00`.

The frame is `C12347E0 001F 00 3F 0013 FFFFFFFF6DB6D861451F11F19716723CBE7E00B1 0000`.
