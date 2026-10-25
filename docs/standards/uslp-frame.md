# USLP Transfer Frame — CCSDS 732.1-B-3

**Crate:** `aldis-uslp` · **Reference:** `specs/pdf/732x1b3e1.pdf` ([how to download](../../specs/README.md)) · **Acronyms:** [glossary](../glossary.md)

## 1. Wire format (§4.1, Annex D)

```
Non-truncated:  Primary Header (7–14) ‖ [Insert Zone] ‖ [Security Header] ‖ TFDF ‖ [Security Trailer] ‖ [OCF (4)] ‖ [FECF (2)]
Truncated:      Truncated Primary Header (4) ‖ [Security Header (6)] ‖ TFDF ‖ [Security Trailer (16)]
TFDF          = TFDF Header (1 or 3) ‖ TFDZ
```

**Primary Header (non-truncated)**

| Bits | Field | Size | Notes |
|---|---|---|---|
| 0–3 | TFVN | 4 | `1100` |
| 4–19 | Spacecraft ID | 16 | |
| 20 | Source-or-Destination ID | 1 | 0 = SCID is source, 1 = destination |
| 21–26 | VCID | 6 | 63 = OID |
| 27–30 | MAP ID | 4 | 0–15; OID frames use 0 |
| 31 | End of Frame Primary Header Flag | 1 | 1 = truncated header (Annex D) |
| 32–47 | Frame Length (total − 1) | 16 | up to 65 536 octets |
| 48 | Bypass/Sequence Control Flag | 1 | 0 = sequence-controlled, 1 = expedited |
| 49 | Protocol Control Command Flag | 1 | 1 = TFDF carries protocol control (COP / SDLS) |
| 50–51 | Reserve Spares | 2 | `00` |
| 52 | OCF Flag | 1 | |
| 53–55 | VCF Count Length | 3 | 0–7 octets |
| 56… | VCF Count | 0–56 | mod 2^(8·len) |

Flag combinations (Table 4-1): `00` sequence-controlled user data, `01` **reserved**, `10` expedited user data, `11` protocol control commands (sequence-controlled service).

**TFDF Header:** TFDZ Construction Rule (3 bits) ‖ UPID (5 bits) ‖ [First Header / Last Valid Octet Pointer (16 bits)].

| Rule | TFDZ | Meaning | Pointer |
|---|---|---|---|
| `000` | fixed | Packets spanning frames | FHP (`0xFFFF` = no packet starts or ends) |
| `001` | fixed | Start of MAPA_SDU / VCA_SDU | LVO (`0xFFFF` = SDU continues) |
| `010` | fixed | Continuing portion of MAPA_SDU / VCA_SDU | LVO (`0xFFFF` = SDU continues) |
| `011` | variable | Octet stream | — |
| `100` | variable | Starting segment | — |
| `101` | variable | Continuing segment | — |
| `110` | variable | Last segment | — |
| `111` | variable | No segmentation (one MAPA/VCA SDU, complete packets, control commands, or Prox-1 SPDUs) | — |

The TFDZ is fixed-length on a VC whose VC Transfer Frame Type is fixed-length, and variable-length otherwise (§4.1.4.2.2.1.1), so a VC's frame type fixes which rules it may carry (USLP-016) and whether the pointer is present (USLP-005).

## 2. Configuration (managed parameters, §5)

| Level | Parameters |
|---|---|
| Physical channel | frame type (fixed/variable), frame length (fixed) or max length (variable), Insert Zone present/length (fixed only), FECF present, max frames per C&S data unit, max repetitions |
| Master channel | frame type, SCID (0–65535), valid VCIDs (0–62, +63), OCF required (fixed only) |
| Virtual channel | frame type (fixed when the physical or master channel is fixed, Table 5-3 note 2), VCID, VCF count length for sequence-controlled and expedited QoS, COP in effect (COP-1/COP-P/none), MAP IDs, truncated frame length (6–32; only on a variable-length VC, Annex D1.2.1; at least 28 with SDLS, Annex D1.2.4), SDU type, OCF allowed (variable) / required (fixed), repetitions, max TFDF completion delay, max inter-frame delay, `SecurityLayout` (per VC/MAP) |
| MAP | MAP ID, SDU type (Packet / MAPA_SDU / Octet stream), UPID |

### 2.1 What the codec keeps

`aldis_uslp::config` keeps the parameters the codec reads; the engines keep the rest (multiplexing schemes, repetitions, delays, COP in Effect, SDU types). The decoder, the writers, the delimiter, and `validate_frame` look the channels up through `VcLookup`, which `VcTable<V, M>` implements with fixed capacities (M2 D14).

| Type | Parameters | Rejected with `InvalidConfig` (field) |
|---|---|---|
| `PhysicalChannelConfig` | Transfer Frame Type, Transfer Frame Length (the frame length when fixed, the maximum when variable), Insert Zone length (0 = absent), FECF | a length above 65 536 (`frame_len` when fixed, `max_frame_len` when variable; §4.1.2.7 note 4) or below 9, even when no VC is configured (the 7-octet Primary Header, the 1-octet TFDF Header of a variable-length frame, and one TFDZ octet; CMN-041, OI-44 item 2; a fixed-length VC has a 3-octet TFDF Header, so `VcTable::new` needs at least 11 octets for one without VCF Count, Insert Zone, or FECF); an Insert Zone on a variable-length channel (`insert_zone_len`; Table 5-1 note 5, USLP-008) |
| `VcConfig` | SCID (its Master Channel), VCID, VC Transfer Frame Type, VCF Count Length for each QoS (0–7 octets, `VcfCountLen`), OCF use (`Absent`, `Allowed` on a variable-length VC, `Required` on a fixed-length VC), truncated frame length (6–32, `TruncatedFrameLen`), where the SDLS lengths are set (`SecurityScope::Vc(layout)` or `PerMap`) | `Allowed` on a fixed-length VC or `Required` on a variable-length one (`ocf_flag`; Table 5-3); a truncated frame length on a fixed-length VC (`truncated_frame_len`; Annex D1.2.1, USLP-002); VCID 63 on a variable-length VC (`transfer_frame_type`; USLP-016) or with `PerMap` (`vc_id`); a Security Header above 64 octets (API-071) or a Security Trailer above 65 535 octets (`security_header_len`, `security_trailer_len`; the limit of the trailer is the 16-bit capacity of `SecurityLayout`, not a Blue Book value) |
| `MapConfig` | SCID, VCID, MAP ID, Security Header and Trailer lengths when the VC sets them per MAP (Table 6-3) | — |
| `VcTable::new` | the channels of one Physical Channel | a variable-length VC on a fixed-length Physical Channel (`transfer_frame_type`; Table 5-3 note 2); VCID 63 on a variable-length Physical Channel (`vc_id`; Table 5-1 note 4, USLP-009); a VC or MAP given twice, a MAP whose VC is not given or is VCID 63, or a MAP with SDLS lengths on a VC with `SecurityScope::Vc` (`vc_id`, `map_id`); a channel whose longest non-truncated frame leaves no TFDZ octet (`frame_data_field`; CMN-041); a truncated frame length below 28 on a VC that uses SDLS, by its own layout or a MAP's (`truncated_frame_len`; Annex D1.2.4, USLP-012); more than `V` VCs or `M` MAPs (`CapacityExceeded`) |

The valid VCIDs of a Master Channel are its `VcConfig` entries, and VCID 63 on a fixed-length Physical Channel (Table 5-2 note 2: "the binary value of 'all ones' (i.e., 63) is always valid"). On a fixed-length Physical Channel, VCID 63 of a Spacecraft ID that has a VC is therefore valid without an entry ([maintainer decision](../process/decisions.md) of 2026-10-25): the decoder, the delimiter, the validator, and `encode_oid` treat it as a fixed-length VC with the defaults of `VcParams::new`, that is no VCF Count, no OCF, and no SDLS fields; an entry for VCID 63 sets these parameters (OCF Required, count lengths, Security lengths) and is also accepted. On a variable-length Physical Channel VCID 63 stays invalid, with or without an entry, because OID frames are not generated for variable-length frames (Table 5-1 note 4, USLP-009): reason 5 when decoding, `InvalidConfig` (`vc_id`) when configuring. The valid MAP IDs of a VC other than 63 are its `MapConfig` entries; VCID 63 carries no MAP channel, and its MAP ID is checked as part of the OID header (§3.1, reason 17). The Master Channel's "Inclusion of OCF Required" is expressed by giving each of its fixed-length VCs `OcfUse::Required`. A protocol control frame (flags `11`) has no Security Header or Trailer (§6.3.1). A truncated frame uses SDLS when the layout that applies to its VC or MAP is not empty, and then has the fixed 6-octet Security Header and 16-octet Security Trailer of Annex D1.2.3 (USLP-012, API-075). 732.1-B-3 §6.3.2.2 sends the reader to "D1.2.5" for the truncated header of a frame with SDLS; D1.2.5 is the rule that truncated and non-truncated frames are not mixed in one CLTU, and the header is D1.3, which the codec follows (a typo of the Blue Book, found by the second review, [m2-vectors.md](../verification/reviews/m2-vectors.md), USLP finding 7).

**Order of the configuration checks (a project choice).** The Blue Book gives none. The values are checked in the order they are built, and the first failure is reported: (1) the Physical Channel (the frame length, then the Insert Zone); (2) each VC's own values, in the order of the `VcConfig` row above (count lengths, then `ocf_flag`, then the truncated frame length, then VCID 63 against its frame type and `PerMap`); (3) each MAP's own values (MAP ID range); (4) the table `VcTable::new`, VCs first, in order, then MAPs, then the truncated frame length that SDLS makes too short. Vectors `uslp-cfg-048`…`055` pin it.

**OID data.** The decoder does not check the data of an OID frame: 732.1-B-3 §4.1.4.1.10 requires the pseudo-noise sequence, which depends on the generator's position across frames (CMN-021) and so cannot be checked by a stateless decoder; receivers discard it. The OID header fields are checked (reason 17).

**VCF Count Length per QoS.** USLP-004 takes the VCF Count Length per QoS (Table 5-3 has one for each), although §4.1.2.11.4 calls it invariant for a VC ([OI-46](../09-open-issues.md)).

## 3. Requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| USLP-001 | The decoder SHALL reject TFVN ≠ `1100` and the reserved flag combination `01`. | §4.1.2.2.2, Table 4-1 | V |
| USLP-002 | If the End of Frame Primary Header Flag = 1, the decoder SHALL decode a truncated frame. Its total length SHALL come from the VC configuration (`truncated_frame_length`, 6–32 octets). Truncated frames are allowed only on a VC whose VC Transfer Frame Type is variable-length: the VC configuration SHALL reject a truncated frame length on a fixed-length VC with `InvalidConfig`, and a truncated frame on a configured VC that has no truncated frame length SHALL be `InvalidField` (End of Frame Primary Header Flag) in both modes (OI-35 item 2). A VC that has a truncated frame length carries only truncated frames (Annex D1.2.1, D1.2.4 note; OI-48): `encode`, the two-phase writer, and `aldis_uslp_encode`, `aldis_uslp_writer_begin`, and `aldis_uslp_writer_finish` SHALL reject a non-truncated frame on it with `InvalidField` (End of Frame Primary Header Flag), and the decoder, `validate_frame` (reason 18), `peek_length`, and the delimiter SHALL treat a non-truncated frame on it as an error, in both modes, since this is a configuration rule and not a reserved value. No VC accepts both kinds, so a bit error in the flag always turns a frame into one its VC refuses, and cannot make a non-truncated frame pass as a truncated one without an FECF (`hz:HZ-USLP-02`). The decoder SHALL treat the frame as expedited user data with a 1-octet TFDF header and no Insert Zone, OCF, or FECF. Its construction rule is one of `011`–`111` (USLP-016). Annex D1.4.1.2 gives rule `111` and the UPID "Mission Specific Information-1" only in notes, so the decoder SHALL expose both and check neither beyond USLP-016, in both modes ([open issues](../09-open-issues.md) OI-35 item 1). The C view of a decoded truncated frame SHALL report `kind` as expedited (`ALDIS_USLP_KIND_EXPEDITED`), the processing of Annex D1.3.1 note 3, and not as the 0 of the members only a full frame has (OI-52). | Annex D1.2.1, D1.2.2, D1.3.1 note 3, D1.3.2, D1.4.1.2 | V |
| USLP-003 | For fixed-length physical channels, the frame length SHALL equal the configured length. For variable-length channels, the Frame Length field SHALL govern and the decoder SHALL consume exactly that many octets. | §4.1.2.7; Table 5-1 | V |
| USLP-004 | The VCF Count SHALL be decoded with the length in the header. A VCF Count Length that differs from the VC's configured count length for the frame's QoS (sequence-controlled or expedited, by the Bypass/Sequence Control Flag) SHALL be `LengthMismatch` in both modes, because the field sets where the header ends (API-032; [open issues](../09-open-issues.md) OI-33). | §4.1.2.11–12, §4.1.2.11.4; Table 5-3 | V |
| USLP-005 | The TFDF header SHALL be 3 octets, with the First Header/Last Valid Octet Pointer, for construction rules `000`, `001`, and `010`, and 1 octet, without it, for `011`–`111`. With USLP-016 the pointer is present exactly when the VC's TFDZ is fixed-length ([open issues](../09-open-issues.md) OI-05, resolved). | §4.1.4.2.1.2, §4.1.4.2.2.2.1–3, §4.1.4.2.4.1, §4.1.4.2.4.3, §4.1.4.2.4.5; Annex C3.1; Annex D1.4.1.2 | V |
| USLP-006 | For rule `000`, the FHP SHALL be classified as `PacketStartsAt(n)` or `NoPacketStartOrEnd` (`0xFFFF`). For rules `001`/`010`, the LVO SHALL be classified as `LastValidOctet(n)` or `SduContinues` (`0xFFFF`). n beyond the TFDZ SHALL be `Malformed`. | §4.1.4.2.4 | V |
| USLP-007 | The UPID SHALL be exposed as a raw 5-bit value plus a typed interpretation from a table generated from the **SANA UPID registry**. Values SHALL NOT be hard-coded from memory (OI-06). Unknown UPIDs SHALL decode successfully as `Upid::Other(n)` in both modes and raise no anomaly ([open issues](../09-open-issues.md) OI-33). | §4.1.4.2.3 | V |
| USLP-008 | The Insert Zone SHALL be permitted only on fixed-length physical channels (configuration error otherwise). | Table 5-1 note 5 | T |
| USLP-009 | OID frames SHALL have VCID 63, MAP ID 0, rule `001`, UPID = "Only Idle Data", LVO = last octet of the TFDZ, and a TFDZ filled from the `OidGenerator`. OID frames SHALL be generated only on fixed-length physical channels. | §4.1.4.1.5–12; Table 5-1 note 4 | V |
| USLP-010 | The writer SHALL compute Frame Length, VCF Count Length (from configuration and QoS), the TFDF header, and the FECF. It SHALL take the TFDZ as given: completing a partly filled fixed-length TFDZ with an Encapsulation Idle Packet is the packet generator's (LNK-013; [open issues](../09-open-issues.md) OI-34). | §4.1.4.3.4 | V |
| USLP-011 | Truncated and non-truncated frames SHALL NOT be mixed in one C&S data unit (enforced by the TC/USLP multiplexing engine, [04](../04-engines.md)). | Annex D1.2.5 | T |
| USLP-012 | With SDLS on truncated frames, the layout SHALL be 6/16 octets and the truncated frame length SHALL be ≥ 28 (API-075). | Annex D1.2.3–4 | T |
| USLP-013 | When the TFDZ carries COP-1 control commands (protocol control flag = 1), the TFDZ SHALL be decodable with the TC control-command decoder (TC-005). | Table 4-1 | V |
| USLP-014 | When the Synchronization and Channel Coding Sublayer below is TC Synchronization and Channel Coding, `validate_frame(&config)` SHALL implement the **USLP Frame Validation Checks**: (a) expected TFVN; (b) one of the expected MCIDs (TFVN + SCID); (c) header values consistent with the features configured for the spacecraft; (d) the octets present consistent with the Frame Length field or, for a truncated frame, with the VC's truncated frame length; (e) FECF matches if present. Each failure SHALL be reported with a distinct reason. | §4.3.11.3 | V |
| USLP-015 | When the Synchronization and Channel Coding Sublayer below is TC Synchronization and Channel Coding, `FrameDelimiter` SHALL implement the **Frame Delimiting and Fill Data Removal Procedure**. Given the octets of one decoded data unit, it SHALL yield successive frames by Frame Length. If the remaining octets are fewer than a frame's declared length, they SHALL be discarded with an event (`IncompleteFrame`). Trailing octets that cannot form a valid header SHALL be treated as fill and discarded. When a header's End of Frame Primary Header Flag is 1, the delimiter SHALL read the VCID from the truncated header and take the frame's length from that VC's truncated frame length (Annex D1.3.2), then continue with the next frame; an unknown VC, or one with no truncated frame length, SHALL end the data unit with a reason, and so SHALL a non-truncated header on a VC that has a truncated frame length (OI-48), whose Frame Length is not trusted. Mixing truncated and non-truncated frames of different VCs in one data unit is not checked on reception, since USLP-011 is a sending rule ([open issues](../09-open-issues.md) OI-35 item 8); of one VC, it cannot occur, since a VC carries only one kind (USLP-002, OI-48). | §4.3.11.1.6, §4.3.11.2 | V |
| USLP-016 | The TFDZ Construction Rule SHALL match the TFDZ type that the VC Transfer Frame Type sets: `000`–`010` on a fixed-length VC, and `011`–`111` on a variable-length VC, which includes every truncated frame. VCID 63 counts as fixed-length, because OID frames exist only on fixed-length channels (USLP-009). The decoder SHALL reject any other rule with `InvalidField` (TFDZ Construction Rule) in both modes, before it reads a pointer, and the writer SHALL reject it with `InvalidField` ([open issues](../09-open-issues.md) OI-35 item 3). | §4.1.4.2.2.1.1, §4.1.4.2.2.2.1–8, Table 4-3; Annex D1.2.1 | V |

USLP-011 (no mixing of truncated and full frames in one C&S data unit) is a sending rule of the multiplexing engine (M4); the codec does not check it on reception, except that one VC never carries both (USLP-002, OI-48).

### 3.1 Check order and validation reasons

`UslpFrame::decode` and `validate_frame` run one sequence of checks (`aldis_uslp::view`), in the order of the USLP Frame Validation Checks of §4.3.11.3.4, then the checks of the TFDF Header, and stop at the first failure. `validate_frame` takes the octets of exactly one frame, as the delimiter yields them, and reports a `Rejection`: the reason below and the decoder's error. `decode` accepts octets after a variable-length or truncated frame (API-011); on a fixed-length Physical Channel it requires exactly the configured frame length (API-012). Each reason has a stable numeric code, shared with the C ABI (M2 D14). Reasons 4 and 17 occur only in strict mode; in lenient mode the deviation is an anomaly flag (§4).

A non-truncated frame (End of Frame Primary Header Flag `0`), in check order:

| Code | Reason | Check | Condition | Decoder error (field) |
|---|---|---|---|---|
| 13 | `LengthMismatch` | (d) | fewer than 4 octets, then fewer than 7 (the length comes before the version and the Spacecraft ID, a project choice pinned for a 5-octet input by `uslp-inv-045`, `-046` and `uslp-val-038`…`040`) | `Incomplete` (`needed` 4, then 7) when decoding; `LengthMismatch` (`frame_len`) when validating |
| 1 | `UnexpectedVersion` | (a) | TFVN ≠ `1100` (§4.1.2.2.2) | `UnsupportedVersion` (`frame_version`), detail expected 12 |
| 2 | `UnexpectedSpacecraft` | (b) | SCID not among the SCIDs of the configured VCs | `UnknownChannel` (`spacecraft_id`) |
| 3 | `ReservedFlags` | (c) | Bypass/Sequence Control Flag `0`, Protocol Control Command Flag `1` (Table 4-1; USLP-001) | `ReservedValue` (`protocol_control_command_flag`) |
| 4 | `ReservedSpare` | (c) | Reserve Spares ≠ `00`, strict mode (§4.1.2.9.2) | `ReservedValue` (`uslp_header_spare`) |
| 5 | `InvalidVcid` | (c) | VCID not valid on the Master Channel (VCID 63 is valid on a fixed-length Physical Channel without an entry, Table 5-2 note 2, and never on a variable-length one, Table 5-1 note 4) | `UnknownChannel` (`vc_id`) |
| 6 | `InvalidMapId` | (c) | MAP ID not valid on the VC (not checked on VCID 63) | `UnknownChannel` (`map_id`) |
| 8 | `VcfCountLengthMismatch` | (c) | VCF Count Length ≠ the VC's length for the QoS of the Bypass/Sequence Control Flag (§4.1.2.11.4; USLP-004) | `LengthMismatch` (`vcf_count_len`) |
| 9 | `OcfFlagMismatch` | (c) | OCF Flag `1` on a VC with `OcfUse::Absent`, or `0` on a VC with `Required` (§4.1.5.3–4, Table 5-3) | `InvalidField` (`ocf_flag`) |
| 10 | `FixedLengthMismatch` | (c) | fixed-length Physical Channel: Frame Length + 1 ≠ the configured frame length (§4.1.2.7; USLP-003) | `LengthMismatch` (`frame_len`) |
| 11 | `FrameTooLong` | (c) | variable-length Physical Channel: Frame Length + 1 above its maximum (Table 5-1 note 2) | `OutOfRange` (`frame_len`) |
| 12 | `FrameTooShort` | (c) | Frame Length + 1 leaves no TFDZ octet after the Primary Header with its VCF Count, Insert Zone, Security Header, TFDF Header (3 octets on a fixed-length VC, 1 on a variable-length VC), Security Trailer, OCF, and FECF the frame has ([OI-44](../09-open-issues.md) item 2) | `InvalidField` (`frame_data_field`) |
| 13 | `LengthMismatch` | (d) | octets present ≠ Frame Length + 1 when validating, or on a fixed-length Physical Channel (API-012); fewer than Frame Length + 1 when decoding on a variable-length one | `LengthMismatch` (`frame_len`); `Incomplete` (`needed` = Frame Length + 1) |
| 14 | `FecfMismatch` | (e) | FECF configured and wrong (§4.1.6, Annex B) | `CrcMismatch` |
| 15 | `ConstructionRuleMismatch` | USLP-016 | TFDZ Construction Rule `011`–`111` on a fixed-length VC or `000`–`010` on a variable-length VC (VCID 63 counts as fixed-length) | `InvalidField` (`tfdz_construction_rule`) |
| 16 | `PointerBeyondZone` | USLP-006 | First Header or Last Valid Octet Pointer ≠ `0xFFFF` and not below the TFDZ length (§4.1.4.2.4) | `Malformed` (`fhp_lvo_pointer`) |
| 17 | `OidHeaderMismatch` | §4.1.4.1.8–9 | VCID 63, strict mode: MAP ID ≠ 0, rule ≠ `001`, UPID ≠ "Only Idle Data", or Last Valid Octet Pointer ≠ the last TFDZ octet ([OI-44](../09-open-issues.md) item 1) | `InvalidField` (`map_id`, `tfdz_construction_rule`, `upid`, `fhp_lvo_pointer`, checked in that order) |
| 18 | `NonTruncatedFrameNotAllowed` | (c) | the VC has a truncated frame length, so it carries only truncated frames (Annex D1.2.1, D1.2.4 note; USLP-002; OI-48); checked after reason 6 and before reason 8, in both modes | `InvalidField` (`end_of_frame_primary_header_flag`) |

A truncated frame (End of Frame Primary Header Flag `1`, Annex D), in check order: reasons 13 (fewer than 4 octets), 1, 2, 5, and 6 as above, then

| Code | Reason | Check | Condition | Decoder error (field) |
|---|---|---|---|---|
| 7 | `TruncatedFrameNotAllowed` | (c) | the VC has no truncated frame length (Annex D1.2.1, D1.3.2; USLP-002) | `InvalidField` (`end_of_frame_primary_header_flag`) |
| 13 | `LengthMismatch` | (d)(2) | octets present ≠ the truncated frame length when validating; fewer when decoding | `LengthMismatch` (`frame_len`); `Incomplete` (`needed` = the truncated frame length) |
| 15 | `ConstructionRuleMismatch` | USLP-016 | TFDZ Construction Rule `000`–`010` (Annex D1.2.1) | `InvalidField` (`tfdz_construction_rule`) |

The truncated frame has no Bypass/Sequence Control Flag, Reserve Spares, OCF, or FECF, so reasons 3, 4, 8–12, 14, 16, and 17 do not apply; its rule (`011`–`111`) and UPID are exposed and not checked further (USLP-002, OI-35 item 1). A UPID the registry does not assign is never an error (USLP-007). The FECF is checked before the TFDF Header is read, so a bit error in the rule or the pointer is reported as reason 14.

`peek_length(prefix, channels)` (API-013) needs 4 octets (`Incomplete`, `needed` 4) and a TFVN of `1100` (`UnsupportedVersion`). For a truncated header it returns the VC's truncated frame length, with the errors of reasons 2, 5, and 7 above. For a non-truncated header of a configured VC that has a truncated frame length it returns the error of reason 18. Otherwise it returns the configured frame length of a fixed-length Physical Channel, or, with 6 octets (`Incomplete`, `needed` 6), Frame Length + 1.

### 3.2 Delimiting

`next_frame(rest, channels)` is stateless: it returns the next item of the octets not yet delimited, and the caller skips its length (M2 D14). `FrameDelimiter` iterates with it. An item is a `Frame`; an `IncompleteFrame` (the rest, discarded: it starts with a valid header that declares more octets than remain; §4.3.11.2.3); `NoTruncatedLength` (the rest, discarded: a truncated header whose SCID and VCID name no configured VC, or a VC without a truncated frame length; USLP-015, OI-35 item 8); `NonTruncatedNotAllowed` (the rest, discarded: a non-truncated header, with at least the Frame Length of a valid header, on a configured VC that has a truncated frame length, which carries only truncated frames; USLP-002, OI-48; its Frame Length is not trusted); or `Fill` (the rest, discarded; §4.3.11.2.4). The next octets start a **valid header** when there are at least 4 of them and the TFVN is `1100`, and either the End of Frame Primary Header Flag is `1` (the length is the VC's truncated frame length, Annex D1.3.2), or it is `0`, at least 7 octets remain, and Frame Length + 1 is at least the Primary Header with its VCF Count, a 1-octet TFDF Header, and one TFDZ octet: 9 + the VCF Count Length (§4.1.2.7, §4.1.2.11); a valid non-truncated header of a configured VC that has a truncated frame length is a `NonTruncatedNotAllowed` item and not a `Frame`. Otherwise the rest is `Fill`. This is the rule [OI-41](../09-open-issues.md) item 2 chose for TC ([OI-44](../09-open-issues.md) item 3). The Fill Data pattern `01010101` has TFVN `0101` and is never a valid header. A frame the delimiter yields still goes through `validate_frame`.

### 3.3 Writers

`encode` and the two-phase `UslpFrameWriter` write a non-truncated frame, `encode_truncated` a truncated one, and `encode_oid` an OID frame (`encode_oid_galois` with the Galois form of the generator, which gives the same frame, CMN-022); each computes the Frame Length, the OCF Flag, the VCF Count Length (from the VC and the QoS of the Bypass/Sequence Control Flag), the TFDF Header, and the FECF, and takes the TFDZ, the Insert Zone, and the OCF as given (USLP-010, API-021). An OID frame's TFDZ comes from the caller's `OidGenerator`, its rule is `001`, its UPID "Only Idle Data", and its Last Valid Octet Pointer the last TFDZ octet (USLP-009). Security Headers and Trailers are written as zeros for the SDLS layer to fill (API-072). The writers check in this order and reject, with nothing written (API-022):

| Condition | Error (field) |
|---|---|
| SCID or VCID not configured | `UnknownChannel` (`spacecraft_id`, `vc_id`) |
| VCID 63 in `encode`, `UslpFrameWriter`, or `encode_truncated`; another VCID in `encode_oid` | `InvalidField` (`vc_id`) |
| `encode_oid`: MAP ID ≠ 0, or protocol control flags | `InvalidField` (`map_id`, `protocol_control_command_flag`) |
| MAP ID not configured on the VC (not on VCID 63) | `UnknownChannel` (`map_id`) |
| `encode_truncated` on a VC without a truncated frame length | `InvalidField` (`end_of_frame_primary_header_flag`) |
| `encode` or `UslpFrameWriter` (`begin`, `resume`) on a VC with a truncated frame length (OI-48) | `InvalidField` (`end_of_frame_primary_header_flag`) |
| a TFDZ Construction Rule of the other TFDZ type (USLP-016) | `InvalidField` (`tfdz_construction_rule`) |
| a VCF Count that does not fit the VC's VCF Count Length for the QoS | `OutOfRange` (`vcf_count`) |
| an OCF on a VC with `OcfUse::Absent`, or none on a VC with `Required` | `InvalidField` (`ocf_flag`) |
| an Insert Zone of another length than the Physical Channel's | `LengthMismatch` (`insert_zone_len`) |
| an empty TFDZ | `InvalidField` (`frame_data_field`) |
| a TFDZ that does not fill a fixed-length frame or a truncated frame exactly | `LengthMismatch` (`frame_data_field`) |
| a frame longer than a variable-length Physical Channel allows | `OutOfRange` (`frame_len`) |
| a First Header or Last Valid Octet Pointer neither `0xFFFF` nor below the TFDZ length | `OutOfRange` (`fhp_lvo_pointer`) |
| `out` shorter than the frame | `BufferTooSmall` |

Until `UslpFrameWriter::finish`, the buffer holds a Primary Header whose TFVN is `1111`, which every decoder rejects with `UnsupportedVersion` and the delimiter treats as fill (API-023). `UslpFrameWriter::resume`, with the arguments of `begin`, makes the same checks and rewrites the unfinished header, the Insert Zone, and the TFDF Header without zero-filling the Security Header and Trailer, so a writer that keeps no state between its calls, such as the C ABI's `aldis_uslp_writer_finish`, can finish the frame (c-abi.md §11).

## 4. Anomaly flags (lenient mode)

`UslpAnomaly` flags. The bit index is the flag's position in `AnomalySet::bits()` and is shared with the C ABI. It never changes once `aldis-uslp` is released. Before OI-33 the flags were listed without bit indices, so withdrawing `VcfLengthMismatchForQos` (an error in both modes, USLP-004) and `UnknownUpid` (reported by `Upid::Other(n)`, USLP-007) reserves no bit. Version, length, and FECF errors remain errors in both modes (API-032).

| Bit | Flag | Deviation | Source |
|---|---|---|---|
| 0 | `ReserveSparesSet` | Reserve Spares (bits 50–51) ≠ `00` | §4.1.2.9.2 |
| 1 | `OidHeaderMismatch` | on VCID 63: MAP ID ≠ 0, TFDZ Construction Rule ≠ `001`, UPID ≠ "Only Idle Data", or Last Valid Octet Pointer ≠ the last TFDZ octet, fixed-value fields of an idle data unit (API-032); the view keeps the values ([OI-44](../09-open-issues.md) item 1, resolved by the maintainer on 2026-10-24, #98) | §4.1.4.1.8–9 |

## 5. Test-vector categories

`uslp/fixed_packets_rule000`, `uslp/fixed_mapa_rules001_010`, `uslp/variable_rules011_111`, `uslp/vcf_count_lengths_0_to_7`, `uslp/truncated`, `uslp/truncated_sdls_layout`, `uslp/oid`, `uslp/insert_zone`, `uslp/protocol_control_cop1`, `uslp/delimit_multiple_frames_with_fill`, `uslp/validation_failures`, `uslp/invalid`, and `uslp/config` (the configuration errors of §2.1).
