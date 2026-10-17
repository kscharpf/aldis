# 00 — Introduction

## 1. Purpose

These libraries add **CCSDS space data system protocols** to two kinds of application:

- **Flight software:** bare-metal and RTOS spacecraft processors with static memory and no heap.
- **Ground systems:** mission control systems, ground-station front ends, simulators, test equipment, and analysis tools.

A single **Rust reference implementation** is the source of truth for behavior. It is exposed to C and C++ through a stable **C ABI**, and later to Java, C#, Python, and Go through bindings over that ABI. Full **native C and C++ implementations** are planned so that single-language applications don't need a Rust toolchain. They must implement this same specification and pass the same conformance vectors (see [07](07-test-vectors.md)).

## 2. v1 scope

v1 covers the **core packet and link standards**:

| Standard | Title | Library coverage in v1 |
|---|---|---|
| 133.0-B-2 | Space Packet Protocol | Full codec; sequence-count and idle-packet helpers |
| 133.1-B-3 | Encapsulation Packet Protocol | Full codec (all four header lengths) |
| 132.0-B-3 | TM Space Data Link Protocol | Full frame codec; sending/receiving engines; SDLS hooks |
| 232.0-B-4 | TC Space Data Link Protocol | Full frame codec; CLCW; control commands; frame delimiting and validation; MAP segmentation and blocking; SDLS hooks |
| 732.0-B-5 | AOS Space Data Link Protocol | Full frame codec incl. Frame Header Error Control, M_PDU, B_PDU, Insert Zone; Issue ≤4 compatibility; engines; SDLS hooks |
| 732.1-B-3 | Unified Space Data Link Protocol | Full frame codec incl. truncated frames and all TFDZ construction rules; engines; SDLS hooks |
| 232.1-B-2 | Communications Operation Procedure-1 | FOP-1 and FARM-1 as sans-I/O state machines, exact to the state tables |
| 301.0-B-4 | Time Code Formats | CUC, CDS, CCS, and ASCII A/B codecs with P-field handling |

### 2.1 Non-goals for v1

The following are **out of scope for v1** and are planned in the [roadmap](08-roadmap.md):

- Synchronization and channel coding (131.x, 231.0): ASM, CLTU, randomizers, RS/convolutional/LDPC/turbo coding. **v1 operates on whole Transfer Frames.** The integrator's radio or modem, or a later release, does the coding.
- SDLS cryptography (355.0/355.1) and the 352.0 algorithms. v1 provides only the **hooks** that reserve and place the Security Header and Trailer (see [03 §7](03-api-model.md#7-security-hooks-sdls-reservation)).
- CFDP, DTN (BP/LTP), IP over CCSDS, SLE/CSTS, navigation messages, Proximity-1, COP-P.
- UTC↔TAI conversion and leap-second tables (see [time codes](standards/time-codes.md) §6).
- Concrete OS transports inside the core. The optional framework supplies transport *traits* and reference adapters (see [05](05-framework.md)).

## 3. Users and use cases

| Actor | Typical use |
|---|---|
| Flight software engineer | Build TM/AOS/USLP downlink frames from Space Packets; receive TC/USLP uplink frames through FARM-1; insert CLCWs into the OCF |
| Ground software engineer | Decode downlink frames and extract packets; drive FOP-1 to send telecommands reliably; time-tag data |
| Ground-station / front-end developer | Validate frames, track frame counts, route virtual channels, strip OCFs |
| Test and simulation engineer | Generate spec-exact frames and packets, inject errors, replay recordings |
| Language-binding maintainer | Wrap the C ABI idiomatically while preserving behavior verified by the vectors |

## 4. Terminology

CCSDS terms (Transfer Frame, Virtual Channel, MAP, FHP, OCF, FECF, CLCW, …) are used as in the Blue Books. Terms specific to this specification:

| Term | Meaning |
|---|---|
| **Codec** | Stateless function or type that encodes or decodes one CCSDS data unit. Bytes in, fields/bytes out. Layer 1. |
| **View** | Zero-copy, read-only decoded representation of a data unit that borrows the caller's buffer. |
| **Writer** | Encoder that writes a data unit into a caller-supplied buffer, optionally in two phases (reserve payload, then finalize). |
| **Engine** | Deterministic, stateful protocol machine with a **sans-I/O** interface. It never performs I/O, reads clocks, allocates, or spawns. Layer 2. |
| **Framework** | Optional Layer 3 that drives engines using caller-implemented transport and clock traits. |
| **Managed parameter** | A CCSDS parameter set by management, not signalled in-band, e.g. frame length or FECF presence. Represented as validated **configuration** objects. |
| **Strict / Lenient decoding** | Strict rejects any deviation from the standard. Lenient accepts deviations in fields the standard says are reserved or ignored, and reports them as **anomalies**. |
| **Profile** | A language-specific binding of this specification (Rust, C ABI, …). |
| **Instant** | Caller-supplied monotonic time: unsigned 64-bit microseconds since an arbitrary epoch. |

## 5. Requirement conventions

### 5.1 Normative language

**SHALL** / **SHALL NOT** mark mandatory requirements. **SHOULD** marks a recommendation. **MAY** marks an option. Text not in a requirement table is informative.

### 5.2 Requirement identifiers

Each requirement has a stable ID: `<AREA>-<NNN>`. IDs are never reused. A deleted requirement keeps its ID with status *Deleted*.

| Area | Document |
|---|---|
| GEN | [02 Design rules](02-design-rules.md) |
| API | [03 API model](03-api-model.md) |
| LNK | [04 Engines](04-engines.md) |
| FW | [05 Framework](05-framework.md) |
| VER | [06 Verification](06-verification.md) |
| TV | [07 Test vectors](07-test-vectors.md) |
| CMN | [Common link features](standards/common-link-features.md) |
| TIME | [Time codes](standards/time-codes.md) |
| SPP | [Space Packet](standards/space-packet.md) |
| EPP | [Encapsulation Packet](standards/encapsulation-packet.md) |
| TM | [TM frames](standards/tm-frame.md) |
| TC | [TC frames and CLCW](standards/tc-frame.md) |
| AOS | [AOS frames](standards/aos-frame.md) |
| USLP | [USLP frames](standards/uslp-frame.md) |
| COP | [COP-1](standards/cop1.md) |
| RS | [Rust profile](profiles/rust.md) |
| CABI | [C ABI profile](profiles/c-abi.md) |

### 5.3 Requirement tables

Requirements appear in tables with these columns:

| Column | Content |
|---|---|
| ID | Requirement identifier |
| Requirement | The SHALL statement |
| Source | CCSDS clause (e.g. `133.0 §4.1.3.5.2`) or `—` for library-design requirements |
| Verif. | **T** = unit/integration test · **V** = shared test vector · **F** = fuzzing/property test · **A** = analysis (tooling, e.g. panic-freedom link check) · **I** = inspection/review |

Every requirement SHALL be traced to at least one verification artefact (see VER-020).

## 6. References

- CCSDS Blue Books listed in §2. They are not redistributed; fetch them into `specs/pdf/` as described in [`specs/README.md`](../specs/README.md).
- CCSDS 320.0-M (SCID assignment) and the SANA registries: Packet Version Numbers, APIDs, Encapsulation Protocol IDs, USLP Protocol IDs (UPIDs), Time Systems.
- RFC 2119 (requirement keywords). The verification documents cite further tool references.
