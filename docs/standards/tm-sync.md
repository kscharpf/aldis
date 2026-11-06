# TM synchronization: ASM, CADU, pseudo-randomizer — CCSDS 131.0-B-6 §9, §10

**Implemented in:** `aldis-sync` (Layer 1, [M5 plan](../milestones/m5-c-abi-completion.md) D2) · **Reference:** `specs/pdf/131x0b6ec1.pdf` ([how to download](../../specs/README.md)) · **Acronyms:** [glossary](../glossary.md)

This document specifies the part of 131.0-B-6 that sits between the Transfer Frame and the physical channel when no error-correcting code and no slicing are used: the Attached Sync Marker and the Channel Access Data Unit (§9, with §3.4 and §11), and the pseudo-randomizer (§10). The rest of 131.0 (R-S, Turbo, and LDPC coding, slicing, the convolutional code) is out of v1 (roadmap §7); convolutional coding alone is covered only in so far as the marker and the randomizer sit around it (§4.2.1, §4.2.2).

## 1. Normative basis and scope

**Term.** 131.0 §9 calls the marker before a codeword the Code Synchronization Marker (CSM) and states that, when frames are not sliced, this one marker does the work of the ASM too and either name may be used (§9.2.2, §3.4.2 a, §2.2.5). This document says **ASM**: with no code and no slicing there is one marker per frame, and the frame protocols (132.0, 732.0, 732.1) and the glossary already use ASM. The glossary records CSM as the other name.

**Where it applies.** 132.0 §2.4.1 (TM), 732.0 §2.4.1 (AOS), and 732.1 §2.4.1 (USLP) name 131.0 as the Synchronization and Channel Coding Sublayer that delivers *fixed-length*, delimited frames. USLP uses 131.0 only with fixed-length frames (Physical Channel Transfer Frame Type `Fixed Length`, 732.1 §2.4.1); its variable-length frames go with 231.0 (TC coding, v1.1) and are not covered here. TC frames are not covered either. (132.0 and 732.0 cite earlier issues of 131.0; this document rests on 131.0-B-6 as downloaded.)

| Frame protocol | Frame length with 131.0 | Source |
|---|---|---|
| TM | any whole number of octets, at most 2048 | 131.0 §11.1, §11.3, §11.4 |
| AOS | any whole number of octets, at most 65536 | 131.0 §11.1, §11.3, §11.4 |
| USLP, fixed length | any whole number of octets, at most 65536 | 131.0 §11.1, §11.3, §11.4 |

The length is fixed for a mission phase on a physical channel (§11.2.1) and does not include the marker (§11.2.1 note). A frame length also has a project limit (TM-001); the lower of the two applies.

**The unit.** What follows the ASM is called the *unit*: the (randomized) Transfer Frame. A CADU is the ASM followed by the randomized unit (§9.1.2, Table 9-1: the marker plus the randomized frame, in the uncoded and convolutional rows). Aldis builds and searches CADUs of this form only.

**Order of operations.** Sending end: the frame is randomized, then the ASM is attached (§3.4.1 c–d, §10.2.2, §10.2.3, §10.3.4 note 1). Receiving end: the ASM is found, the unit after it is derandomized, then the frame is decoded (§3.4.2, §10.2.3, §10.3.4). The ASM is not randomized (§10.3.4 note 1).

## 2. The marker and the CADU

The marker is 32 bits, `1ACFFC1D` in hexadecimal, first transmitted bit first (Figure 9-1: bit 0 is the first transmitted bit): the octets `1A CF FC 1D`, each sent most significant bit first. The longer markers of §9.3.2–§9.3.4 belong to Turbo and LDPC coding and are out of scope (OI-82, decided).

## 3. The pseudo-randomizer

The randomizer exclusive-ORs the bits of the unit with a fixed sequence (§10.2.1). Bit 1 of the unit meets bit 1 of the sequence, the first transmitted bit first (§10.3.2); within an octet that is the most significant bit.

The sequence of §10.4.1 is 131071 bits long and comes from h(x) = x¹⁷ + x¹⁴ + 1, initialised to `11000111000111000` at the start of every unit (§10.4.3). The Blue Book gives the first 40 bits (§10.4.3 note 2: `0001 1100 0111 0001 1011 1001 0001 1011 1010 1001`) and a register diagram (Figure 10-2), which §10.5 gives as one possible generator rather than the definition. To make the sequence unambiguous, this reading writes it as a recurrence over the sequence a(0), a(1), …:

- a(0)…a(16) are the 17 seed digits read from the right: 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1;
- a(n) = a(n−3) ⊕ a(n−17) for n ≥ 17 (the exponents 17 and 14 of h(x)).

This reproduces the 40 bits of note 2 and has period 131071 (checked during spec preparation by running the recurrence; the blind model of WP-2 and the vectors check it independently, TV-023). The sequence restarts at every unit (§10.4.3), so a unit's mask never depends on the units before it. A unit of up to 65536 octets is 524288 bits, so the sequence wraps up to four times inside one unit, at a bit position that is not an octet boundary.

## 4. The ASM search engine

131.0 gives the marker and notes only that a receiver acquires synchronization by recognizing the marker pattern at its regular spacing, and that it may then confirm it with further checks (§9.2 note). It gives no error tolerance, no lock rule, and no bit alignment. [M5 plan](../milestones/m5-c-abi-completion.md) D3 settles the first two: the engine finds the ASM at any bit offset, with a configured number of tolerated bit errors, as a sans-I/O engine over octet chunks whose states are a closed enum (RS-021); it has no frame-sync flywheel and no lock thresholds. What D3 left open the maintainer decided on 2026-11-05: [OI-80](../09-open-issues.md) (the first matching position wins; a tolerance of 0 to 15), [OI-81](../09-open-issues.md) (search only: no lock, verification, or polarity search), and [OI-82](../09-open-issues.md) (both randomizer sequences; the 32-bit ASM only). The requirements below record those decisions.

*Engine.* The engine reads the stream in chunks of octets. It has two states, `Searching` and `Collecting` (SYNC-012). In `Searching` it keeps the last 31 stream bits and, at every bit position once 32 bits have been read since the search began, compares the last 32 bits with the ASM; a match needs at most the configured number of differing bits. A match starts `Collecting`: the engine takes the next 8 × frame length bits as the unit (§9.2.1: the ASM immediately follows the end of the preceding unit, so units are contiguous and of fixed length, §11.2.1), shifts them to an octet boundary, and, when the unit is complete, returns it as an action and goes back to `Searching` with an empty window. The unit's own bits are never searched, since the unit is an opaque, randomized frame.

*Capacities.* `CHUNK` is the largest input chunk in octets and `FRAME` the largest frame length the instance accepts; both are compile-time capacities (GEN-011). `FRAME` octets hold the unit that is being collected.

## 5. Requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| SYNC-001 | The ASM SHALL be the 32-bit pattern `1ACFFC1D`, transmitted first bit first: the octets `1A CF FC 1D`, each most significant bit first. The documents, the API, and the C ABI call it the ASM; CSM is the same marker (glossary). | 131.0 §9.3.1, Fig. 9-1; §9.2.2 | V |
| SYNC-002 | The SYNC functions SHALL apply to the TM, AOS, and fixed-length USLP Transfer Frames of one physical channel, with no slicing and no coding, or with convolutional coding alone around the CADU. A configuration SHALL name the frame protocol and the frame length in octets, and SHALL be rejected with `InvalidConfig` naming `frame_len` when the length is 0 or above the maximum of the protocol (TM 2048, AOS and USLP 65536), and with `CapacityExceeded` when it is above the instance's `FRAME`. | 131.0 §11.1, §11.2.1, §11.3, §11.4; 132.0 §2.4.1; 732.0 §2.4.1; 732.1 §2.4.1 | T/V |
| SYNC-003 | ASM attach SHALL write the CADU: the ASM, then the frame octets as given, with nothing between and no change to the frame. The output is 4 + frame length octets. A frame whose length is not the configured frame length SHALL fail with `LengthMismatch` naming `frame_len`; an output area shorter than the CADU SHALL fail with `BufferTooSmall{needed}`; nothing is written on error. | 131.0 §9.2.1, §9.1.2 Table 9-1, §3.4.1 d | T/V |
| SYNC-004 | The randomizer SHALL exclusive-OR the buffer in place, bit for bit, with the sequence the caller selects (SYNC-005, SYNC-020), the first bit of the buffer with the first bit of the sequence. It SHALL be one function for both directions: applying it twice restores the buffer. It SHALL accept a buffer of any length, including 0, and SHALL NOT fail. It SHALL be a function of the buffer alone: the sequence restarts at the first octet of every call. | 131.0 §10.2.1, §10.3.2–§10.3.3, §10.4.3 | T/V |
| SYNC-005 | The 131071-bit sequence SHALL be a(0)…a(16) = 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1 and a(n) = a(n−3) ⊕ a(n−17) for n ≥ 17, the sequence of h(x) = x¹⁷ + x¹⁴ + 1 started from the seed `11000111000111000`. It SHALL repeat after 131071 bits, with the repetition running on across the end of a period inside one buffer. Its first 40 bits SHALL be those of the Blue Book note. Bit i of the sequence is bit (7 − i mod 8) of octet ⌊i/8⌋. | 131.0 §10.4.1, §10.4.3, §10.4.3 note 2 | V |
| SYNC-006 | The randomizer and the attach function SHALL NOT be combined by the codec: the caller randomizes the frame, then attaches the ASM, so that the ASM is never randomized; on receipt it derandomizes the unit that the engine returns. The search engine SHALL NOT derandomize. | 131.0 §10.3.4 note 1, §3.4.1, §10.2.2–§10.2.3 | I |
| SYNC-007 | The search engine SHALL find the ASM at any bit offset of the stream. A position matches when the number of bits in which the last 32 stream bits differ from the ASM is at most the configured tolerance (`asm_errors`); the stream is read most significant bit first. A match needs 32 bits read since the engine started, was reset, or returned its last unit. The tolerance is 0 to 15 bit errors (decided in OI-80). | 131.0 §9.2 note, §9.3.1; M5 plan D3 | T/V |
| SYNC-008 | Where several positions could match, the engine SHALL take the first one in stream order (the earliest start bit) without waiting to see a better one (OI-80). | — (OI-80) | T/V |
| SYNC-009 | After a match the engine SHALL collect exactly 8 × frame length bits as the unit, in order, and SHALL NOT search them for an ASM. It SHALL return the unit with its first bit as the most significant bit of its first octet, whatever the bit offset of the ASM. | 131.0 §9.2.1, §11.2.1, §3.4.2 | T/V |
| SYNC-010 | The result of a stream SHALL NOT depend on how the stream is divided into chunks: an ASM or unit that crosses a chunk boundary, at any bit, SHALL give the same units, in the same order, with the same fields, as the stream fed in one chunk. A chunk of 0 octets SHALL change nothing and return no action. | 131.0 §9.2 note | T/V/F |
| SYNC-011 | Each completed unit SHALL be one action unit: the range of the unit in the output area (API-061), `errors` (the number of ASM bits that differed, 0 up to the tolerance), and `skipped_bits` (the number of stream bits between the end of the previous unit, or the start or reset of the stream, and the first bit of the ASM, saturating at the largest `u64`). The actions of one input come in stream order, and their ranges do not overlap. A partial unit at the end of a chunk SHALL produce no action; it is state. | — (API-057, API-061) | T/V |
| SYNC-012 | The engine SHALL have exactly the states Searching and Collecting, as a closed enum matched exhaustively without a wildcard arm (RS-021: the data decide the next state). The state SHALL hold no output (API-059), the last 31 stream bits, the count of bits read since the search began, the octets of the partial unit in storage of `FRAME` octets (API-060), and the counters of SYNC-015. | RS-021; M5 plan D3 | T/I |
| SYNC-013 | The action bound of an input (API-057) SHALL be N = ⌈c / (frame length + 4)⌉ for a chunk of c octets, because the units that end inside the chunk end at least 8 × (frame length + 4) bits apart. The compile-time bound the engine documents SHALL be N = ⌈`CHUNK` / 5⌉, the value for a frame length of 1 octet (SYNC-002). | API-057, API-062 | T |
| SYNC-014 | The output-area bound of an input (API-062) SHALL be frame length × ⌈c / (frame length + 4)⌉ octets for a chunk of c octets, which is at most `CHUNK` + `FRAME`; the engine SHALL document the latter as the size that fits every input. A chunk larger than `CHUNK` SHALL fail with `OutOfRange` naming `chunk_len`. An output area smaller than the units the input completes SHALL fail with `BufferTooSmall{needed}`, `needed` being the exact number of octets, and leave the state unchanged (API-058). | API-058, API-061, API-062 | T |
| SYNC-015 | The engine SHALL expose a read-only status (API-056): the number of units returned, the total number of bits skipped (saturating), the state, and the number of octets of the partial unit. A Reset input SHALL discard the partial unit and the window, restart the skipped-bit count, and keep the counters; a new configuration SHALL take effect only through it, as LNK-003 does for the link engines. | API-056; LNK-003 | T |
| SYNC-016 | The tolerance SHALL be a type with a checked constructor; a value above 15 SHALL be rejected with `InvalidConfig` naming `asm_errors`. The configuration SHALL keep the protocol, the frame length, and the tolerance together, valid by construction (RS-020). | RS-020; OI-80 (decided) | T |
| SYNC-017 | For every frame of the configured length, every bit offset 0–7 and any prefix of octets without a match, attach, randomize, derandomize, and search SHALL return the frame, with `errors` 0 and `skipped_bits` equal to the prefix in bits; with up to the tolerance bits of the ASM flipped it SHALL still return the frame, with `errors` equal to the flips (VER-023). | 131.0 §9.2, §10.3.4; VER-023 | T/F |
| SYNC-018 | The engine and the codec functions SHALL be deterministic functions of their inputs, with no clock, randomness, or I/O, and SHALL NOT panic on any input (GEN-020, GEN-030–032). | GEN-020, GEN-030 | T/F/A |
| SYNC-019 | The engine SHALL NOT verify the spacing of the units, hold a lock, or search for the complemented ASM (OI-81). It returns every unit it finds with `skipped_bits`, from which the caller checks the spacing. | 131.0 §9.2 note, §2.2.5; M5 plan D3 (OI-81) | I |
| SYNC-020 | The randomizer SHALL offer a second, legacy sequence, selected by the caller with the sequence choice of the call (`randomizer_sequence`), for older systems (OI-82, decided): 255 bits long, from h(x) = x⁸ + x⁷ + x⁵ + x³ + 1, with its register set to all ones at the start of every unit. As a recurrence: b(0)…b(7) = 1 and b(n) = b(n−1) ⊕ b(n−3) ⊕ b(n−5) ⊕ b(n−8) for n ≥ 8, repeating after 255 bits. Its first 40 bits SHALL be those of the Blue Book note. Everything in SYNC-004 holds for it; there is no default, the caller selects the sequence. A value of the choice that is neither sequence (a C ABI input) SHALL fail with `InvalidConfig` naming `randomizer_sequence`. | 131.0 §10.4.2, §10.4.3, §10.4.3 note 2 | V |

## 6. Field codes

The configuration parameters that an error names are in the field catalogue ([03](../03-api-model.md) §9.1), block 250–299 (owner `sync`): `asm_errors` (250), `sync_protocol` (251), `chunk_len` (252), `randomizer_sequence` (253). The frame length is the existing `frame_len` (68), which `sync` now names too. `sync_protocol` is named by the C ABI when it is given a value that is none of TM, AOS, USLP; the Rust type cannot hold one.

## 7. Test-vector categories

The case groups are the directories of `test-vectors/sync/`; see [test vectors](../07-test-vectors.md). They are planned for WP-2: CADUs per protocol (TM, AOS, USLP) at the lengths of the table in §1; the randomizer sequence over a full period and across the wrap inside a 65536-octet unit; the ASM search at every bit offset, with errors up to and past the tolerance; an ASM and a unit split across chunks at every bit; error cases (`BufferTooSmall`, `LengthMismatch`, `OutOfRange`, `InvalidConfig`).

## 8. Readings left to the maintainer

[OI-80](../09-open-issues.md), [OI-81](../09-open-issues.md), and [OI-82](../09-open-issues.md) listed what 131.0 leaves to the implementation, or what the scope of v1 had not decided; the maintainer decided all three on 2026-11-05 and the requirements that name them (SYNC-007, SYNC-008, SYNC-016, SYNC-019, SYNC-020) record the decisions.
