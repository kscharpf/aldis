# Glossary

This page expands the acronyms and abbreviations used in the specification, the crate READMEs, and the code, and defines the words the project itself uses. It has three parts:

1. [Acronyms and abbreviations](#1-acronyms-and-abbreviations), from the CCSDS Blue Books and the tools Aldis uses.
2. [Aldis terms](#2-aldis-terms): the vocabulary of this repository.
3. [Blue Book documents](#3-blue-book-documents): the CCSDS document numbers cited, with their titles.

**How to read it.** The definitions are written for a reader new to CCSDS and are paraphrases, not quotations: the Blue Books govern (see [README](README.md#source-material)). *Defined in* gives the document number and clause where the term is defined or first specified; fetch the documents with [`specs/fetch.sh`](../specs/README.md). Where Blue Books spell a term differently, or Aldis uses a name the Blue Books do not, the row says so. Requirement IDs (e.g. CMN-010) point into the [requirement areas](#requirement-areas).

**Anchors.** Every entry has an anchor made from its name: lower case, letters and digits kept, every other run of characters replaced by one `-`. So OCF is [`#ocf`](#ocf), M_PDU is [`#m-pdu`](#m-pdu), GMAP ID is [`#gmap-id`](#gmap-id), C&S is [`#c-s`](#c-s), N(S) is [`#n-s`](#n-s), and the golden trace is [`#golden-trace`](#golden-trace). A requirement area has the prefix `area-` ([`#area-tm`](#area-tm)), and a Blue Book the prefix `ccsds-` ([`#ccsds-132-0-b-3`](#ccsds-132-0-b-3)). Link to an entry from another document as `glossary.md#ocf`.

## 1. Acronyms and abbreviations

**Index:**
[AD](#ad) · [AOS](#aos) · [APID](#apid) · [ASan](#asan) · [ASCII](#ascii) · [ASM](#asm) · [AUTOSAR](#autosar) · [B_PDU](#b-pdu) · [BC](#bc) · [BCD](#bcd) · [BCH](#bch) · [BD](#bd) · [BP](#bp) · [C ABI](#c-abi) · [C&S](#c-s) · [CCS](#ccs) · [CCSDS](#ccsds) · [CDS](#cds) · [CERT](#cert) · [CFDP](#cfdp) · [cFE](#cfe) · [cFS](#cfs) · [CI](#ci) · [CI_LAB](#ci-lab) · [CLCW](#clcw) · [CLTU](#cltu) · [COP-1](#cop-1) · [COP-P](#cop-p) · [CRC](#crc) · [CSM](#csm) · [CUC](#cuc) · [DOY](#doy) · [DTN](#dtn) · [EPI](#epi) · [EPP](#epp) · [FARM-1](#farm-1) · [FARM-B](#farm-b) · [FCLTU](#fcltu) · [FDU](#fdu) · [FEC](#fec) · [FECF](#fecf) · [FFI](#ffi) · [FHEC](#fhec) · [FHP](#fhp) · [FOP-1](#fop-1) · [FSH](#fsh) · [FSR](#fsr) · [GF](#gf) · [GMAP ID](#gmap-id) · [GVCID](#gvcid) · [IN_SDU](#in-sdu) · [IPoC](#ipoc) · [IZ](#iz) · [K](#k) · [LDPC](#ldpc) · [LFSR](#lfsr) · [LTP](#ltp) · [LVO](#lvo) · [M_PDU](#m-pdu) · [MAP](#map) · [MAP ID](#map-id) · [MAPA](#mapa) · [MAPP](#mapp) · [MC](#mc) · [MC/DC](#mc-dc) · [MC_FSH](#mc-fsh) · [MC_OCF](#mc-ocf) · [MCID](#mcid) · [MISRA](#misra) · [MSB](#msb) · [MSRV](#msrv) · [N(R)](#n-r) · [N(S)](#n-s) · [NN(R)](#nn-r) · [OCF](#ocf) · [OID](#oid) · [P-field](#p-field) · [PC](#pc) · [PDU](#pdu) · [PLCW](#plcw) · [PN](#pn) · [POD](#pod) · [Prox-1](#prox-1) · [PVN](#pvn) · [PW, NW, W](#pw) · [QoS](#qos) · [RAF](#raf) · [RAII](#raii) · [RF](#rf) · [RS](#rs) · [RTTI](#rtti) · [SAFETY comment](#safety-comment) · [SANA](#sana) · [SCID](#scid) · [SDLS](#sdls) · [SDU](#sdu) · [SLE](#sle) · [SPP](#spp) · [SS](#ss) · [T-field](#t-field) · [T1_Initial](#t1-initial) · [TAI](#tai) · [TC](#tc) · [TFDF](#tfdf) · [TFDZ](#tfdz) · [TFVN](#tfvn) · [TM](#tm) · [TT](#tt) · [Type-1 Report](#type-1-report) · [Type-A, -B, -C, -D](#type-a) · [UBSan](#ubsan) · [UPID](#upid) · [USLP](#uslp) · [UTC](#utc) · [V(R)](#v-r) · [V(S)](#v-s) · [VC](#vc) · [VC_FSH](#vc-fsh) · [VC_OCF](#vc-ocf) · [VCA](#vca) · [VCF Count](#vcf-count) · [VCID](#vcid) · [VCP](#vcp)

### 1.1 Organizations, protocols, and procedures

| Term | Expansion | Meaning | Defined in | In Aldis |
|---|---|---|---|---|
| <a id="ccsds"></a>**CCSDS** | Consultative Committee for Space Data Systems | The international committee of space agencies that publishes the standards Aldis implements. Its normative standards are the Blue Books. | Acronym annex of every Blue Book, e.g. 132.0 Annex B | Every requirement cites a CCSDS clause; documents listed in §3 and [`specs/`](../specs/README.md) |
| <a id="sana"></a>**SANA** | Space Assigned Numbers Authority | The registry that publishes the values CCSDS assigns, such as packet version numbers, protocol IDs, and spacecraft IDs. Implementations take those values from the registry, not from a standard's text. | 132.0 Annex B; 133.0 Annex B2; 732.1 Annex E2 | Registry values never come from memory (OI-06); USLP UPIDs from a committed registry snapshot (USLP-007, [M2 plan](milestones/m2-frame-codecs.md) D13); EPI sets supplied by the caller (EPP-010) |
| <a id="tm"></a>**TM** | Telemetry | The CCSDS data link protocol for downlink: fixed-length Transfer Frames that carry packets or fixed-size user data on several virtual channels. | 132.0 Annex B; the protocol is 132.0 | `aldis-tm`; area [TM](#area-tm) |
| <a id="tc"></a>**TC** | Telecommand | The CCSDS data link protocol for commanding: variable-length Transfer Frames, made reliable by COP-1 when required. | 232.0 Annex B; the protocol is 232.0 | `aldis-tc`; area [TC](#area-tc) |
| <a id="aos"></a>**AOS** | Advanced Orbiting System (732.0-B-5 and 732.0-B-4 Annex B); Advanced Orbiting Systems (133.0, 133.1, 131.0, 355.0, 732.1, 700.1) | A data link protocol with fixed-length frames, an optional Frame Header Error Control and Insert Zone, and data fields that carry packets (M_PDU), a bitstream (B_PDU), or fixed-size user data. The Blue Books disagree on the final "s". | 732.0 Annex B; the protocol is 732.0 | `aldis-aos`; area [AOS](#area-aos); editions `Issue5` and `Issue4Compatible` (AOS-001) |
| <a id="uslp"></a>**USLP** | Unified Space Data Link Protocol (732.1, 231.0, 355.0); "Unified Space Link Protocol" in 131.0 Annex D and 700.1 Annex A | The newest CCSDS data link protocol: fixed- or variable-length frames for either direction, a wider Spacecraft ID, MAPs, a header on the data field (TFDF), and truncated frames. | 732.1 Annex G; the protocol is 732.1 | `aldis-uslp`; area [USLP](#area-uslp) |
| <a id="spp"></a>**SPP** | Space Packet Protocol | The protocol of the Space Packet, the basic CCSDS packet, which carries application data named by an APID. | 133.0 Annex D; the protocol is 133.0 | `aldis-spp`; area [SPP](#area-spp) |
| <a id="epp"></a>**EPP** | Encapsulation Packet Protocol | The protocol of the Encapsulation Packet, a packet with a short, variable header that carries other protocols' data units over CCSDS links. | 133.0 Annex D; 133.1 Annex B; the protocol is 133.1 | `aldis-epp`; area [EPP](#area-epp) |
| <a id="sdls"></a>**SDLS** | Space Data Link Security | The CCSDS protocol that authenticates and encrypts Transfer Frames, using a Security Header before the data field and a Security Trailer after it. | 132.0 Annex B; 355.0 Annex C; the protocol is 355.0 | v1 only reserves the fields: `aldis_core::security` (`SecurityLayout`, `SecurityHook`, `NoSecurity`), API-070…075; cryptography is roadmap v2 |
| <a id="cop-1"></a>**COP-1** | Communications Operation Procedure-1 (COP: 232.0 Annex B) | The closed-loop retransmission procedure that gives TC, and USLP, a sequence-controlled service: FOP-1 sends and retransmits, FARM-1 accepts and reports in the CLCW. | 232.1 (whole document); 232.0 §2.3.3 | `aldis-cop1`; area [COP](#area-cop) |
| <a id="fop-1"></a>**FOP-1** | Frame Operation Procedure-1 (FOP: 232.1 Annex A) | The sending end of COP-1, usually on the ground: numbers Type-AD frames, keeps copies until the CLCW acknowledges them, and retransmits on timeout or request. | 232.1 §5 | `aldis_cop1::fop1` (`Fop1`), COP-010…017 |
| <a id="farm-1"></a>**FARM-1** | Frame Acceptance and Reporting Mechanism-1 (FARM: 232.1 Annex A) | The receiving end of COP-1, usually on the spacecraft: accepts frames in sequence and reports its state in the CLCW. | 232.1 §6 | `aldis_cop1::farm1`, COP-030…036 |
| <a id="farm-b"></a>**FARM-B** | Frame acceptance and reporting mechanism, expedited service (732.1 Annex G) | Used in Aldis only in the CLCW's FARM-B Counter, which counts accepted Type-B frames. | 232.0 §4.2.1.9; 232.1 §6.1.6 | COP-034 |
| <a id="cop-p"></a>**COP-P** | Communications operation procedure Proximity (732.1 Annex G) | The retransmission procedure of Proximity-1. USLP can run it instead of COP-1, and it reports through the PLCW. | 211.0-B-6, cited by 732.1 ref. [10]; not in `specs/` | Not in v1 ([00 §2.1](00-introduction.md#2-v1-scope)); roadmap v3 |
| <a id="prox-1"></a>**Prox-1** | Proximity-1 | The CCSDS Proximity-1 Space Link Protocol. 732.1 Annex C relates its Version-3 frames to USLP. | 133.0 Annex D; 732.1 Annex C | Not in v1; named in the USLP construction rule table ([uslp-frame.md](standards/uslp-frame.md) §1) |

### 1.2 Channels and identifiers

| Term | Expansion | Meaning | Defined in | In Aldis |
|---|---|---|---|---|
| <a id="pc"></a>**PC** | Physical channel. Aldis abbreviation; the Blue Books spell it out (732.1 has PCID, Physical Channel identifier) | A single stream of bits over a space link in one direction. It carries the frames of one or more master channels. | 132.0 §1.6.1.3, §2.1.3 | `PhysicalChannelConfig` ([03 §4](03-api-model.md#4-configuration-objects)); ChannelMux and FrameReceiver run per PC ([04](04-engines.md)) |
| <a id="mc"></a>**MC** | Master Channel | All frames on a physical channel with the same MCID: one spacecraft's frames of one protocol version. | 132.0 §2.1.3; 232.0 §2.1.3; 732.1 §2.1.3 | `MasterChannelConfig`; MC frame count (LNK-032) |
| <a id="vc"></a>**VC** | Virtual Channel | A logical channel within a physical channel, so that several data streams with different needs can share one link. | 132.0 §2.1.2.1, §2.1.3 | `aldis_core::ids::VcId`; `VirtualChannelConfig`; most engines run per VC ([04](04-engines.md)) |
| <a id="map"></a>**MAP** | Multiplexer Access Point | A sub-channel of a TC or USLP virtual channel, so that several users can share one VC. The frames of one MAP form a MAP Channel. | 232.0 §2.1.3; 732.1 §2.1.3 | `MapConfig`; MapSegmenter and MapReassembler ([04](04-engines.md)) |
| <a id="map-id"></a>**MAP ID** | Multiplexer Access Point Identifier | The field that names the MAP: in the TC Segment Header, and in the USLP primary header. | 232.0 §4.1.3.2.2.3; 732.1 §4.1.2.5 | `aldis_core::ids::MapId`; field code 3 ([03 §9.1](03-api-model.md#91-field-codes)) |
| <a id="mcid"></a>**MCID** | Master Channel Identifier | The TFVN and the SCID together, which name a master channel. | 132.0 §2.1.3, §4.1.2.2 | Frame validation check (b) (TC-021, USLP-014) |
| <a id="gvcid"></a>**GVCID** | Global Virtual Channel Identifier | The MCID and the VCID together, which name a virtual channel on a link. | 132.0 §2.1.3; 232.0 §2.1.3 | Not a type: a VC is configured by its VCID within a master channel configuration |
| <a id="gmap-id"></a>**GMAP ID** | Global MAP ID (232.0 §2.1.3, 732.1 §2.1.3), or Global Multiplexer Access Point Identifier (232.0 Annex B, 732.1 Annex G); 355.0 writes GMAP_ID | The GVCID and the MAP ID together, which name a MAP Channel. | 232.0 §2.1.3; 732.1 §2.1.3 | USLP segments of one SDU stay on one GMAP ID (LNK-042) |
| <a id="scid"></a>**SCID** | Spacecraft Identifier | The frame header field that names the spacecraft. Values are assigned under CCSDS 320.0 and SANA; the width depends on the protocol and, for AOS, the edition. | 132.0 §4.1.2.2.3; 232.0 §4.1.2.5; 732.0 §4.1.2.2.3, §4.1.2.5.4; 732.1 §4.1.2.2.3 | `aldis_core::ids::SpacecraftId<BITS>` (API-002); the reference application's fictional SCID (OI-26) |
| <a id="vcid"></a>**VCID** | Virtual Channel Identifier | The header field that names the VC within its master channel. | 132.0 §4.1.2.3; 232.0 §4.1.2.6; 732.0 §4.1.2.3; 732.1 §4.1.2.4 | `VcId`; field code 2 |
| <a id="tfvn"></a>**TFVN** | Transfer Frame Version Number | The first field of every Transfer Frame header, which says which data link protocol the frame belongs to. | 132.0 §4.1.2.2.2; 232.0 §4.1.2.2; 732.0 §4.1.2.2.2; 732.1 §4.1.2.2.2 | Checked first by every frame decoder (TM-002, TC-002, AOS-011, USLP-001) |
| <a id="qos"></a>**QoS** | Quality of Service | Whether data gets the sequence-controlled service (complete, in order, retransmitted by a COP) or the expedited service (sent once). | 232.0 §2.2.2, §2.3.3; 732.1 §2.2.3 | Chosen per request (LNK-043); carried in engine output metadata (API-054); USLP VCF Count length per QoS (USLP-004) |

### 1.3 Frame fields and frame data units

| Term | Expansion | Meaning | Defined in | In Aldis |
|---|---|---|---|---|
| <a id="ocf"></a>**OCF** | Operational Control Field | A small fixed-size field near the end of a TM, AOS, or USLP frame that carries a real-time report to the other end of the link, most often the CLCW of COP-1. AOS sets its presence per VC by management; TM and USLP signal it in the header. | 132.0 §4.1.5; 732.0 §4.1.5; 732.1 §4.1.5 | `aldis_core::ocf::Ocf` (planned, [M2 plan](milestones/m2-frame-codecs.md) WP-1; CMN-010, CMN-011); `OcfSource` (LNK-033); `Event::Ocf` (LNK-053). Aldis names the variant that holds a Type-1 Report `Clcw`, although on USLP that report may be a PLCW (CMN-010) |
| <a id="type-1-report"></a>**Type-1 Report**, **Type-2 Report** | — | The two kinds of OCF content, told apart by its first bit. A Type-1 Report serves retransmission control (a CLCW, or on USLP a PLCW); a Type-2 Report is project-specific or an SDLS FSR. | 132.0 §4.1.5.4–5; 732.1 §4.1.5.2 | `Ocf::Clcw`, `Ocf::ProjectSpecific`, `Ocf::SdlsFsr` (CMN-010) |
| <a id="clcw"></a>**CLCW** | Communications Link Control Word | The COP-1 status report that FARM-1 builds and the return link carries in the OCF. FOP-1 reads it to learn what was accepted and what to retransmit. | 232.0 §4.2 | `aldis-tc` module `clcw` (`Clcw`, TC-030, TC-031); FOP-1 `handle_clcw`, FARM-1 `clcw()` (COP-035) |
| <a id="plcw"></a>**PLCW** | Proximity Link Control Word | The COP-P counterpart of the CLCW, carried as a Type-1 Report on USLP. | 732.1 Annex G, §4.1.5.2.1; its format is in 211.0 | No codec in v1; decoded as `Ocf::Clcw(raw)` (CMN-010) |
| <a id="fsr"></a>**FSR** | Frame Security Report | An SDLS status report carried as a Type-2 Report in the OCF. | 732.1 Annex G, §4.1.5.2.2; 132.0 §4.1.5.5 defers its format to 355.1-B-1, which is not in `specs/` | Opaque `Ocf::SdlsFsr(raw)` (CMN-010) |
| <a id="fecf"></a>**FECF** | Frame Error Control Field | An optional 16-bit CRC at the end of a frame that lets the receiver detect transmission errors. All four data link protocols define it the same way. | 132.0 §4.1.6; 232.0 §4.1.4; 732.0 §4.1.6; 732.1 §4.1.6, Annex B | `aldis_core::crc` (`crc16`, `verify_fecf`, `write_fecf`), CMN-001…004; error `CrcMismatch` |
| <a id="crc"></a>**CRC** | Cyclic Redundancy Check | The class of error-detecting code that the FECF uses. "CRC-16" in the Aldis documents means the FECF code. | 732.0 Annex B | [Common link features](standards/common-link-features.md) §1 |
| <a id="fhec"></a>**FHEC** | Frame Header Error Control. Aldis abbreviation: 732.0 always spells the name out | An optional Reed-Solomon check field in the AOS primary header that lets the receiver correct a small number of symbol errors in the header fields it covers. | 732.0 §4.1.2.6 | `aldis_aos::fhec` (AOS-013, AOS-014); error `HeaderUncorrectable`; independent model `aldis_testkit::oracle::fhec` |
| <a id="fsh"></a>**FSH** | Frame Secondary Header (132.0 Annex B); 132.0 §4.1.3 calls the field Transfer Frame Secondary Header | An optional fixed-length TM header after the primary header, for data tied to a master or virtual channel. | 132.0 §4.1.3 | TM-003; [tm-frame.md](standards/tm-frame.md) §1–2 |
| <a id="mc-fsh"></a>**MC_FSH**, <a id="vc-fsh"></a>**VC_FSH** | Master Channel Frame Secondary Header; Virtual Channel Frame Secondary Header | The FSH service when the header belongs to a master channel or to one virtual channel. | 132.0 §3.8, §3.5; 355.0 Annex C | TM channel configuration ([tm-frame.md](standards/tm-frame.md) §2) |
| <a id="mc-ocf"></a>**MC_OCF**, <a id="vc-ocf"></a>**VC_OCF** | Master Channel Operational Control Field; Virtual Channel Operational Control Field | The OCF service when the field belongs to a master channel or to one virtual channel. | 132.0 §3.9, §3.6; 355.0 Annex C | LNK-033 |
| <a id="iz"></a>**IZ** | Insert Zone (700.1 Annex A; 732.0 and 732.1 spell it out) | An optional fixed-length zone after the AOS or USLP primary header that carries one IN_SDU in every frame of the physical channel (on USLP, only on fixed-length channels, USLP-008). | 732.0 §4.1.3; 732.1 §4.1.3 | AOS-015, USLP-008; `InsertSource` (LNK-034) |
| <a id="fhp"></a>**FHP** | First Header Pointer (acronym in 732.1 Annex G) | A pointer that gives the position of the first packet starting in a frame's data field: in the TM primary header, the AOS M_PDU header, or the USLP TFDF header. A receiver uses it to find packets again after a lost frame. | 132.0 §4.1.2.7.6; 732.0 §4.1.4.2.2; 732.1 §4.1.4.2.4.3 | TM-008, AOS-017, USLP-006; set by the PacketGenerator (LNK-011), followed by the PacketExtractor (LNK-060) |
| <a id="lvo"></a>**LVO** | Last Valid Octet (pointer). Aldis abbreviation: 732.1 names only the combined First Header/Last Valid Octet Pointer | The same USLP pointer when it marks the last octet of a MAPA_SDU or VCA_SDU in a fixed-length TFDZ. | 732.1 §4.1.4.2.4 | USLP-006; LNK-020, LNK-072 |
| <a id="tfdf"></a>**TFDF** | Transfer Frame Data Field | The USLP data field: a TFDF Header (construction rule, UPID, and an optional pointer) followed by the TFDZ. | 732.1 §4.1.4 | `aldis-uslp` module `tfdf` (USLP-005, USLP-006, USLP-016) |
| <a id="tfdz"></a>**TFDZ** | Transfer Frame Data Zone | The part of the TFDF after its header, which holds the packets, SDU pieces, or idle data. | 732.1 §4.1.4.3 | USLP-010, USLP-016; filled by the PacketGenerator (LNK-011) |
| <a id="m-pdu"></a>**M_PDU** | Multiplexing Protocol Data Unit | The AOS data field format for packets: a header with a First Header Pointer, then the Packet Zone. | 732.0 §4.1.4.2 | `aldis_aos::mpdu` (AOS-003, AOS-017) |
| <a id="b-pdu"></a>**B_PDU** | Bitstream Protocol Data Unit | The AOS data field format for an unstructured bitstream: a header with a pointer to the last valid bit, then the data. | 732.0 §4.1.4.3 | `aldis_aos::bpdu` (AOS-017, AOS-018); BitstreamGenerator (LNK-021) |
| <a id="vcf-count"></a>**VCF Count** | Virtual Channel Frame Count | A per-VC counter of frames sent, which lets the receiver detect lost frames. TM and AOS spell the field out; "VCF Count" is USLP's short form, where the header also gives its length. On its own, VCF names the Virtual Channel Frame service (132.0 §2.2.3.6). | 132.0 §4.1.2.6; 732.0 §4.1.2.4; 732.1 §4.1.2.11–12 | Modular counters in `aldis-core` (CMN-030, CMN-031); USLP-004; LNK-032, LNK-051 |
| <a id="oid"></a>**OID** | Only Idle Data | A frame whose data field holds only idle data, a pseudo-noise fill, sent when no data is ready, so that the frame stream continues. | 132.0 §4.1.4.6; 732.0 §4.1.4.1.5; 732.1 §4.1.4.1.5–10 | `aldis_core::oid::OidGenerator` (CMN-020…024); TM-010, AOS-016, USLP-009; OID frames from the ChannelMux (LNK-030, LNK-036) |
| <a id="pn"></a>**PN** | Pseudo Noise | A deterministic bit sequence that looks random. The data link protocols use one to fill OID frames; the coding sublayers use one to randomize the transmitted bits, which 131.0 §10 calls the pseudo-randomizer and 231.0 §6 the randomizer. | 132.0 Annex B, §4.1.4.6.2 | `OidGenerator`; randomizers are roadmap v1.1 |
| <a id="lfsr"></a>**LFSR** | Linear Feedback Shift Register | The shift-register construction that generates the OID PN sequence, in a Fibonacci or a Galois form. | 132.0 §4.1.4.6.2, Annex D | `OidGenerator`, `OidGeneratorGalois` (CMN-020, CMN-022) |
| <a id="fdu"></a>**FDU** | Frame Data Unit | The content of a TC Type-D frame's data field: user data, preceded by a Segment Header when the VC uses one. An FDU with a Segment Header is a segment. | 232.0 §4.1.3.2 | Input of FOP-1 `request_ad` and `request_bd` ([cop1.md](standards/cop1.md) §2.2); MapSegmenter output (LNK-043) |
| <a id="pdu"></a>**PDU** | Protocol Data Unit | The OSI term for the unit a protocol exchanges with its peer. The Transfer Frame is the PDU of a data link protocol. | 132.0 §1.6.1.1, §1.6.1.3 | In M_PDU and B_PDU |
| <a id="msb"></a>**MSB** | Most Significant Bit | The CCSDS bit convention: bit 0 of a field is the first transmitted and the most significant. | 132.0 Annex B; 133.0 §1.6.3 | GEN-050 |
| <a id="gf"></a>**GF** | Galois Field | A finite field. The FHEC computes over a field of 16 elements. | 131.0 Annex D; 732.0 §4.1.2.6.5 | AOS-013 |

### 1.4 Packets, service data units, and services

| Term | Expansion | Meaning | Defined in | In Aldis |
|---|---|---|---|---|
| <a id="apid"></a>**APID** | Application Process Identifier | The Space Packet header field that names the managed data path the packet belongs to, in practice its application stream; it is unique only within its naming domain. One value is reserved for Idle Packets. | 133.0 §4.1.3.3.4 | `aldis_core::ids::Apid` (`is_idle`); `aldis-spp`; field code 5 |
| <a id="pvn"></a>**PVN** | Packet Version Number | The first header field of a CCSDS packet, which says which packet format follows (Space Packet, Encapsulation Packet, or another one registered with SANA). | 133.0 §4.1.3.2; 133.1 §4.1.2.2; SANA registry (132.0 ref. [6]) | `aldis_core::ids::PacketVersion`; the valid PVNs of a VC (LNK-010; OI-06); field code 4 |
| <a id="epi"></a>**EPI** | 133.1 names the field Encapsulation Protocol ID (§4.1.2.3), but its §2.3 and Annex B expand EPI as Encapsulated Protocol Identifier; Aldis follows the field name | The Encapsulation Packet header field that names the protocol of the data carried. Reserved values mark an Idle Packet, a mission-specific protocol, and a protocol named instead by the Protocol ID Extension field. | 133.1 §2.3, §4.1.2.3, §4.1.2.6 | `aldis_epp::Epi`, `EpiExtension`, `EpiSet`, `EppConfig` (EPP-006, EPP-010); field codes 57, 58 |
| <a id="upid"></a>**UPID** | USLP Protocol Identifier (732.1); "USLP protocol ID" in 700.1 | The TFDF header field that names the protocol or kind of data in the TFDZ, from a SANA registry. | 732.1 §4.1.4.2.3; registry: 732.1 ref. [14] | `aldis-uslp` module `upid` (`Upid`, `Upid::Other`), generated from a SANA snapshot (USLP-007; M2 plan D13) |
| <a id="sdu"></a>**SDU** | Service Data Unit | The OSI term for a unit of data a user hands to a service, which the service delivers unchanged to the user at the other end: a packet, a VCA_SDU, a MAP_SDU, and so on. | 132.0 §1.6.1.1 (from ISO/IEC 7498-1) | Engine `Deliver` actions ([03 §5](03-api-model.md#5-engines-the-sans-io-contract)); MapSegmenter input |
| <a id="in-sdu"></a>**IN_SDU** | Insert Service Data Unit | The fixed-length unit of the AOS and USLP Insert Service, carried in the Insert Zone of every frame. | 732.0 §3.2.7; 732.1 §3.2.8 | `InsertSource` and the `InSduLoss` indication (LNK-034) |
| <a id="vca"></a>**VCA**, **VCA_SDU** | Virtual Channel Access (Service Data Unit) | A service that carries user data units whose format the protocol does not know. TM and AOS carry exactly one per frame data field (LNK-020). | 132.0 §3.4, §3.2.3; 732.0 §3.2.4; 732.1 §3.2.4 | AccessGenerator and AccessReceiver (LNK-020); TM Synchronization Flag ([tm-frame.md](standards/tm-frame.md) §1) |
| <a id="vcp"></a>**VCP** | Virtual Channel Packet (service) | The service that carries packets on a virtual channel. | 132.0 §3.3; 232.0 §3.4; 732.0 §3.3 | PacketGenerator ([04 §2.1](04-engines.md#21-packetgenerator-tm--aos--uslp-fixed-length-vc-packet-and-map-packet-services)) |
| <a id="mapp"></a>**MAPP** | Multiplexer Access Point Packet (service) | The service that carries packets on a MAP. | 232.0 §2.2.3.2, §3.3; 732.1 §3.3 | MapSegmenter, MapReassembler |
| <a id="mapa"></a>**MAPA**, **MAPA_SDU**, **MAP_SDU** | Multiplexer Access Point Access (Service Data Unit) | The service that carries user-defined data units on a MAP. TC names its unit MAP_SDU (MAP Channel Access Service Data Unit), USLP names it MAPA_SDU. | 232.0 §3.2.3, §3.5; 732.1 §3.2.3 | `MapConfig` content; LNK-020, LNK-040, LNK-072 |

### 1.5 COP-1 frames and variables

| Term | Expansion | Meaning | Defined in | In Aldis |
|---|---|---|---|---|
| <a id="type-a"></a>**Type-A**, **Type-B**, **Type-C**, **Type-D** | — | TC frame kinds. Type-A frames pass FARM-1's acceptance check (sequence-controlled service); Type-B frames bypass it (expedited service). Type-D frames carry data; Type-C frames carry control commands. | 232.0 §2.2.2, §4.1.2.3 | TC-002, TC-004, TC-005, TC-008 |
| <a id="ad"></a>**AD**, <a id="bd"></a>**BD**, <a id="bc"></a>**BC** | Acceptance check and Data; Bypass of acceptance check and Data; Bypass of acceptance check and Control | The three valid combinations of the TC Bypass and Control Command flags: sequence-controlled data, expedited data, and COP-1 control commands. | 232.1 Annex A; 232.0 §4.1.2.3, Table 4-1 | Kind of a FOP-1 `Transmit` ([cop1.md](standards/cop1.md) §2.3); COP-010 |
| <a id="n-s"></a>**N(S)** | Frame Sequence Number | The TC header count that FOP-1 gives each Type-AD frame. | 232.0 §4.1.2.8; 232.1 Annex A | `aldis_core::ids::FrameSeqNum`; TC-008, COP-010 |
| <a id="n-r"></a>**N(R)** | Report Value | The CLCW field with the sequence number FARM-1 expects next. | 232.0 §4.2.1.11; 232.1 Annex A | [tc-frame.md](standards/tc-frame.md) §5; COP-012 |
| <a id="nn-r"></a>**NN(R)** | Expected_Acknowledgement_Frame_Sequence_Number | FOP-1's copy of the N(R) of the previous CLCW: the last frame it knows arrived. | 232.1 §5.1.8 | COP-012 |
| <a id="v-s"></a>**V(S)** | Transmitter_Frame_Sequence_Number | The N(S) that FOP-1 will give the next new Type-AD frame. | 232.1 §5.1.3 | COP-010; Set V(S) directive |
| <a id="v-r"></a>**V(R)** | Receiver_Frame_Sequence_Number | The N(S) that FARM-1 expects in the next Type-AD frame. | 232.1 §6.1.7 | COP-030; Set V(R) control command (TC-005) |
| <a id="k"></a>**K** | FOP_Sliding_Window_Width | How many frames FOP-1 may send ahead of the last acknowledged one. | 232.1 §5.1.12 | `sliding_window_k` in `Fop1Config`; COP-015 |
| <a id="pw"></a>**W**, **PW**, **NW** | FARM_Sliding_Window_Width; FARM_Positive_Window_Width; FARM_Negative_Window_Width | FARM-1's window: the range of N(S) values it can handle without lockout, and its parts ahead of and behind V(R). | 232.1 §6.1.8 | `Farm1Config` ([cop1.md](standards/cop1.md) §3.1); COP-030 |
| <a id="t1-initial"></a>**T1_Initial** | Timer_Initial_Value | The start value of FOP-1's retransmission timer, restarted on each Type-AD or Type-BC transmission. | 232.1 §5.1.9 | `t1_initial` in `Fop1Config`; COP-013 |
| <a id="tt"></a>**TT** | Timeout_Type | What FOP-1 does when the timer expires and the transmission limit has been reached. | 232.1 §5.1.10.3 | `timeout_type` in `Fop1Config` |
| <a id="ss"></a>**SS** | Suspend_State | The FOP-1 state recorded when the AD service is suspended, to which it returns on resume. | 232.1 §5.1.11 | COP-014 |

### 1.6 Time codes

| Term | Expansion | Meaning | Defined in | In Aldis |
|---|---|---|---|---|
| <a id="cuc"></a>**CUC** | CCSDS Unsegmented Time Code ([time-codes.md](standards/time-codes.md) §3 shortens it to "CCSDS Unsegmented Code") | A binary count of seconds (coarse time) and binary fractions of a second (fine time) since an epoch. Level 1 uses the CCSDS epoch on the TAI scale; Level 2 an epoch the agency defines. | 301.0 §3.2 | `aldis_time::Cuc`, `CucFormat` (TIME-010…014) |
| <a id="cds"></a>**CDS** | CCSDS Day Segmented Time Code (time-codes.md §4: "CCSDS Day Segmented Code") | A binary count of days since an epoch, milliseconds of the day, and an optional sub-millisecond part. UTC-based. | 301.0 §3.3 | `aldis_time::Cds` (TIME-020…022) |
| <a id="ccs"></a>**CCS** | CCSDS Calendar Segmented Time Code (time-codes.md §5: "Calendar Segmented Code") | A calendar date (month and day, or day of year) and time of day in BCD, with optional decimal fractions of a second. UTC-based. | 301.0 §3.4 | `aldis_time::Ccs` (TIME-030, TIME-034) |
| <a id="ascii"></a>**ASCII** time codes A and B | CCSDS ASCII Calendar Segmented Time Code | Text forms of a UTC calendar time: variation A with month and day, variation B with day of year. | 301.0 §3.5 | `aldis_time::AsciiTime` (TIME-031, TIME-032) |
| <a id="p-field"></a>**P-field** | Preamble Field | The optional leading octets of a time code that identify the code and its layout. It is either sent with the time (explicit) or agreed by management (implicit). | 301.0 §3.1.1 | `aldis_time::PField`, `TimeCodeId`; `decode_with_pfield`, `decode_implicit` (TIME-001…006) |
| <a id="t-field"></a>**T-field** | Time Field | The octets that hold the time value itself. | 301.0 §3.1.2 | Each code's value type in `aldis-time` |
| <a id="tai"></a>**TAI** | International Atomic Time | The atomic time scale, without leap seconds. CUC Level 1 counts TAI seconds. | 301.0 Annex C; §3.2 | TIME-014 |
| <a id="utc"></a>**UTC** | Coordinated Universal Time | The civil time scale: it runs at the TAI rate and is kept near Earth rotation by leap seconds. CDS, CCS, and the ASCII codes are UTC-based. | 301.0 Annex C; §3.3–3.5 | `aldis_time::UtcInstant`; TIME-022, TIME-033; no TAI↔UTC conversion in v1 (OI-10) |
| <a id="bcd"></a>**BCD** | Binary Coded Decimal | Decimal digits stored one per 4-bit nibble, as in the CCS T-field. | 301.0 §3.4 | TIME-030 |
| <a id="doy"></a>**DOY** | Day of year | The CCS calendar variation that gives the day of the year instead of month and day. | 301.0 §3.4.1.2 | `NonZeroDoyHighNibble` (TIME-034) |

### 1.7 Synchronization and channel coding (outside v1)

v1 works on whole Transfer Frames; the integrator's radio or modem does the coding ([00 §2.1](00-introduction.md#2-v1-scope)).

| Term | Expansion | Meaning | Defined in | In Aldis |
|---|---|---|---|---|
| <a id="c-s"></a>**C&S** | Coding and Synchronization (700.1 Annex A; 732.1 §2.1.2.4 for Proximity-1). 131.0, 231.0, and 232.0 call their layer the Synchronization and Channel Coding Sublayer | The sublayer below the data link protocol that adds synchronization markers and error-correcting codes. | 700.1 Annex A; 732.1 §2.1.2.4, Table 5-1 | The CLTU packer groups frames into one C&S data unit (LNK-044); USLP-011 |
| <a id="asm"></a>**ASM** | Attached Sync Marker (131.0 Annex D, 732.1 Annex G); "attached sync mark" in 355.0 Annex C | A fixed bit pattern placed before each frame or codeword so the receiver can find where it starts. | 131.0 §2.2.5, §3 | Roadmap v1.1 |
| <a id="csm"></a>**CSM** | Code Sync Marker (131.0 Annex D), Code Synchronization Marker (131.0 §1.5.1.3) | The marker before each codeword. Without slicing it is the same as the ASM, and 131.0 uses the two names interchangeably. | 131.0 §2.2.5, §9 | Roadmap v1.1 (131.0 §9) |
| <a id="cltu"></a>**CLTU** | Communications Link Transmission Unit | The uplink unit of the TC coding sublayer: a start sequence, an integral number of codewords that carry the coded frame data, and a tail sequence. | 231.0 §1.6.1.3, §5 | Built by the integrator in v1; CLTU packer (LNK-044); `TcUplinkSenderStack` ([05](05-framework.md)) |
| <a id="bch"></a>**BCH** | Bose-Chaudhuri-Hocquenghem | A family of block codes; 231.0 uses one of them for TC codewords, LDPC being the other choice. | 231.0 Annex C1, §3 | Roadmap v1.1 |
| <a id="ldpc"></a>**LDPC** | Low-Density Parity-Check (131.0); Low Density Parity Check (231.0, 700.1) | A family of error-correcting codes defined for both TM and TC. | 131.0 §8; 231.0 §4 | Roadmap v3 |
| <a id="rs"></a>**RS** | Reed-Solomon (131.0 writes R-S) | Error-correcting codes that work on multi-bit symbols. 131.0 uses one for TM codeblocks; the AOS FHEC is a short Reed-Solomon code. Not to be confused with the Rust profile's requirement area [RS](#area-rs). | 131.0 Annex D, §5; 732.0 §4.1.2.6.5 | FHEC (AOS-013); 131.0 Reed-Solomon coding is roadmap v3 |
| <a id="fec"></a>**FEC** | Forward error correcting (700.1 Annex A) | Coding that lets the receiver correct errors without asking for a retransmission. | 700.1 Annex A | Roadmap v3 ("131.0 full FEC") |
| <a id="rf"></a>**RF** | Radio frequency | Appears in the CLCW's No RF Available Flag, which reports whether the radio elements of the physical layer are ready. | 732.1 Annex G; 232.0 §4.2.1.8 | FARM-1 `set_physical_flags` ([cop1.md](standards/cop1.md) §3.2) |

### 1.8 Other CCSDS standards (later scope)

| Term | Expansion | Meaning | Defined in | In Aldis |
|---|---|---|---|---|
| <a id="cfdp"></a>**CFDP** | CCSDS File Delivery Protocol | The CCSDS protocol for transferring files. | 133.0 Annex D; 727.0-B-4 | Roadmap v2 |
| <a id="dtn"></a>**DTN** | Delay Tolerant Networking | Networking designed to tolerate long delays and disrupted links. | 700.1 Annex A | Roadmap v3 |
| <a id="bp"></a>**BP** | Bundle Protocol | A DTN protocol. | 133.0 Annex D | Roadmap v3 |
| <a id="ltp"></a>**LTP** | Licklider Transmission Protocol (133.0 Annex D); "Licklider Transport Protocol" in 700.1 Annex A | A DTN protocol; the roadmap pairs it with BP. | 133.0 Annex D | Roadmap v3 |
| <a id="ipoc"></a>**IPoC** | IP over CCSDS | Carrying Internet Protocol traffic over CCSDS links. | 133.0 Annex D | Roadmap v3 |
| <a id="sle"></a>**SLE** | Space Link Extension | A family of CCSDS ground services for space link data. The Aldis documents name two of them, RAF and FCLTU. | 355.0 Annex C; 700.1 Annex A | Ground interface in [01 §3.3](01-architecture.md#33-ground-mirror-image); roadmap v4 |
| <a id="raf"></a>**RAF** | Return All Frames (SLE-RAF: Space Link Extension Return All Frames) | The SLE service through which a ground system receives the downlink frames. | 700.1 Annex A | [01 §3.3](01-architecture.md#33-ground-mirror-image) |
| <a id="fcltu"></a>**FCLTU** | Forward CLTU | The SLE service through which a ground system hands CLTUs to the station for uplink. | Title of 231.0 ref. [5], "Space Link Extension—Forward CLTU Service Specification" | [01 §3.3](01-architecture.md#33-ground-mirror-image) |

The roadmap ([08 §7](08-roadmap.md#7-later-standards-proposed-order)) also names CSTS and XTCE. No document in `specs/` expands them, so they are not defined here.

### 1.9 Profile and tooling terms

| Term | Expansion | Meaning | Defined in | In Aldis |
|---|---|---|---|---|
| <a id="c-abi"></a>**C ABI** | C application binary interface (general term) | The stable C interface of Aldis, the header `include/aldis/aldis.h`, that the Rust-backed and the native libraries both expose, so that an application can link either without source changes. | [C ABI profile](profiles/c-abi.md) (area CABI) | `aldis-ffi`; CABI-070, CABI-072 |
| <a id="ffi"></a>**FFI** | Foreign function interface (general term; [01 §1](01-architecture.md#1-layering) says "foreign-function boundary") | The boundary at which code in one language calls another. | [01 §1](01-architecture.md#1-layering) | `aldis-ffi`, the only library crate allowed `unsafe` (GEN-071); tested under Miri (VER-026) |
| <a id="safety-comment"></a>**SAFETY comment** | — | The `// SAFETY:` comment that justifies each `unsafe` block, required by GEN-071 and by Clippy's `undocumented_unsafe_blocks`. | GEN-071; [AGENTS.md](../AGENTS.md) | `aldis-ffi`, `difftest/` |
| <a id="mc-dc"></a>**MC/DC** | Modified condition/decision coverage (general term; the repository does not expand it) | A structural coverage criterion stronger than branch coverage, required by some certification standards. | VER-024; OI-09 | Not a gate: reserved for a later certification effort (VER-024, OI-09) |
| <a id="msrv"></a>**MSRV** | Minimum supported Rust version (general term) | The oldest Rust compiler the crates build with. | RS-001; OI-07 | Equal to the pinned toolchain (RS-001) |
| <a id="pod"></a>**POD** | Plain old data (general term) | A struct of plain values with no pointers, constructors, or hidden state, safe to copy byte for byte across the C ABI. | CABI-010, CABI-012 | Every type that crosses the C ABI |
| <a id="raii"></a>**RAII** | Resource acquisition is initialization (general term) | The C++ idiom in which an object's constructor acquires a resource and its destructor releases it. | CABI-072 | Handle types of `aldis.hpp` |
| <a id="rtti"></a>**RTTI** | Run-time type information (general term) | C++ support for `typeid` and `dynamic_cast`, disabled in Aldis builds. | — | `-fno-rtti` |
| <a id="asan"></a>**ASan**, <a id="ubsan"></a>**UBSan** | AddressSanitizer; UndefinedBehaviorSanitizer | Compiler instrumentation that stops a test on a memory error or on undefined behavior. | VER-026 | C test harness of `aldis-ffi` (M5) |
| <a id="cert"></a>**CERT** | The SEI CERT C and C++ Coding Standards | Free secure-coding rule sets that C code (VER-051) follows. | VER-051; OI-22 | |
| <a id="autosar"></a>**AUTOSAR** C++14 | The AUTOSAR C++14 Guidelines | A free C++ rule set considered for native C++ code. | OI-22 | |
| <a id="misra"></a>**MISRA** | MISRA C and MISRA C++ coding guidelines | Rule sets that were considered and dropped: the rulebooks must be purchased and the checkers are commercial (VER-055). | OI-22 | Not used |
| <a id="ci"></a>**CI** | Continuous integration. Aldis convention | Where the specification says a check runs "in CI", it means a check of the [verification run](#verification-run), not a hosted service; GitHub Actions only repeats the checks [nightly](#nightly). | [06 §7](06-verification.md#7-verification-runs-and-the-merge-gate) | `cargo xtask verify` |

### 1.10 Flight software frameworks

Not CCSDS terms: names from NASA's Core Flight System, as its repositories use them (read at `v7.0.1`, [ACFS plan](milestones/acfs-flight-side.md) §1.1).

| Term | Expansion | Meaning | Defined in | In Aldis |
|---|---|---|---|---|
| <a id="cfs"></a>**cFS** | Core Flight System | NASA's open-source flight software framework: the cFE, OSAL, and a PSP, plus applications, released as one bundle. | nasa/cFS (`README.md`) | The cFS flight side of the reference application ([reference-application.md](reference-application.md) §10; APP-110…APP-125) |
| <a id="cfe"></a>**cFE** | Core Flight Executive | The cFS core: executive services (ES), events (EVS), the Software Bus (SB), tables (TBL), and time (TIME), under which cFS applications run. | nasa/cFE | Runs unchanged under the cFS flight side (APP-111); its time is kept out of the trace (OI-36, OI-37) |
| <a id="osal"></a>**OSAL** | Operating System Abstraction Layer | The cFS library that hides the operating system: tasks, queues, timers and timebases, sockets, files, and the console. | nasa/osal | The cFS bridge's TCP socket (APP-118) |
| <a id="psp"></a>**PSP** | Platform Support Package | The cFS layer for one hardware or operating-system platform, such as `pc-linux`; it provides the local clock and the system timebase. | nasa/PSP | pc-linux only (APP-111) |
| <a id="sb"></a>**SB** | Software Bus | The cFE publish-and-subscribe message service: apps send messages by message ID and receive them through pipes. | nasa/cFE (`modules/sb`) | The only channel between the cFS flight side's apps (APP-115) |
| <a id="sch"></a>**SCH**, <a id="sch-lab"></a>**SCH_LAB** | Scheduler; scheduler lab application | A cFS app that sends messages on a fixed schedule driven by a timer; SCH_LAB is the lab version in the cFS bundle. | nasa/sch_lab | Replaced by the lockstep sequencer `refapp_step` (APP-116) |
| <a id="ci-lab"></a>**CI_LAB** | Command ingest lab application | The cFS bundle's command-ingest app: it receives messages over UDP and puts them on the Software Bus unchanged. | nasa/ci_lab | Replaced by `refapp_ci` (APP-114) |
| <a id="to-lab"></a>**TO_LAB** | Telemetry output lab application | The cFS bundle's telemetry-output app: it sends subscribed Software Bus messages over UDP unchanged. | nasa/to_lab | Replaced by `refapp_to` (APP-113) |
| <a id="met"></a>**MET** | Mission Elapsed Time | cFE TIME's count of time since start, from which it forms spacecraft time with the STCF. | nasa/cFE (`modules/time`) | OI-37 |
| <a id="stcf"></a>**STCF** | Spacecraft Time Correlation Factor | The cFE TIME offset added to the MET to give spacecraft time. | nasa/cFE (`modules/time`) | OI-37 |

## 2. Aldis terms

These are conventions of this repository, not CCSDS terms, unless the entry says otherwise.

### 2.1 Project organization

| Term | Meaning | Defined in |
|---|---|---|
| <a id="track"></a>**Track** | Active tracks of v1 work, each owning its own directories: **R**, the Rust reference (`crates/`, `fuzz/`, `exerciser/`, `crosscheck/`, the generated C header); **A**, the reference application (`refapp/`). Track **C** (native C++, `cpp/`, `difftest/`) was retired from v1 and deferred to roadmap Phase 3. | [08-roadmap.md](08-roadmap.md) (table at the top), §6 rule 2 |
| <a id="milestone"></a>**Milestone** | A step of a track with an entry gate and exit criteria. Track R: **M0** Foundations, **M1** Packets & time, **M2** Frame codecs, **M3** COP-1, **M4** Link engines, **M5** C ABI completion, **M6** Framework. Track A: **A0** Scenario, **A1** Packets, **A2** Frames, **A3** COP-1, **A4** Engines, **A5** C and C++, **A6** Framework and target. Track C (retired from v1): C0–C5. Only track R milestones cut releases, named `v0.N.0-mN` (e.g. `v0.2.0-m1`). | [08-roadmap.md](08-roadmap.md) §1–3, §5 |
| <a id="entry-gate"></a>**Entry gate**, **exit criteria** | The results that must be merged on `main` before a milestone starts, and the Area/Criteria table in its plan that says when it is done (measured by `cargo xtask trace --scope <id>`). | [08-roadmap.md](08-roadmap.md) §6 rule 1; the plans in [`milestones/`](milestones/) |
| <a id="milestone-plan"></a>**Milestone plan** | The document in [`milestones/`](milestones/) (`m<N>-…`, `c<N>-…`, `a<N>-…`) written before a milestone starts: starting point, decisions, work packages, exit criteria, risks. | [08-roadmap.md](08-roadmap.md) §6 rule 1 |
| <a id="wp"></a>**WP** (work package) | A numbered unit of work in a milestone plan, e.g. M2 WP-0, sized S, M, or L by relative effort. A WP may be split into pull requests such as WP-0a, WP-0b, WP-0c. The plan's status carries a work-package table, one row per WP or sub-part (WP, State, PRs, Notes); requirement counts come from `cargo xtask trace --scope <id>`, and the definition of done from [checklists.md](milestones/checklists.md). | §3 of each milestone plan, e.g. [M2 plan](milestones/m2-frame-codecs.md) §3 |
| <a id="decision"></a>**Decision** (D-number) | A numbered choice recorded in a milestone plan's Decisions table, with its alternative and status. Numbers are per plan, so cite them with the plan: "M2 plan D13". | §2 of each milestone plan |
| <a id="oi"></a>**OI** (open issue) | A spec question still to be decided or verified, numbered OI-01 upward. Code never resolves one silently: the OI is updated instead. | [09-open-issues.md](09-open-issues.md); [AGENTS.md](../AGENTS.md#sources-of-truth) |
| <a id="branch-prefix"></a>**Branch prefix** | `m<N>/`, `c<N>/`, `a<N>/` for track work; `spec/`, `chore/`, `fix/` for the rest. | [08-roadmap.md](08-roadmap.md) §6 rule 3 |
| <a id="changelog-fragment"></a>**Changelog fragment** | The one file a pull request adds to [`changelog.d/`](../changelog.d/README.md), named after its branch, instead of editing `CHANGELOG.md`. | [changelog.d/README.md](../changelog.d/README.md) |
| <a id="decision-rights"></a>**Decision rights** | Who may decide what: the **maintainer** (the project owner), the **coordinator** (the session that plans the work, dispatches agents, and opens and readies pull requests), or the **agent** of a work package, for each category of decision (`spec`, `scope`, `plan`, `api`, `vectors`, `review`, `process`, `deps`, `release`, `merge`, `housekeeping`, `approval`); how to ask the maintainer, and what continues while a question waits. Not to be confused with a plan's D-numbered [decisions](#decision). | [process/decision-rights.md](process/decision-rights.md); VER-062 |
| <a id="decision-log"></a>**Decision log** | The index of the maintainer's decisions, one line each: `- YYYY-MM-DD · <category> · <#N or chat> · <decision>`. The coordinator appends a line whenever the maintainer decides something; `cargo xtask decisions` checks the format. | [process/decisions.md](process/decisions.md); VER-062 |

### 2.2 Requirements

| Term | Meaning | Defined in |
|---|---|---|
| <a id="requirement-id"></a>**Requirement ID** | `<AREA>-<NNN>`, e.g. SPP-002. IDs are never reused; a deleted requirement keeps its ID with status *Deleted*. A new requirement takes the next free number in its area. | [00 §5.2](00-introduction.md#5-requirement-conventions) |
| <a id="verification-method"></a>**Verif. column** | How a requirement is verified: **T** unit or integration test, **V** shared test vector, **F** fuzzing or property test, **A** analysis by a tool (e.g. the panic-freedom link check), **I** inspection or review. | [00 §5.3](00-introduction.md#5-requirement-conventions) |
| <a id="req-annotation"></a>**`// req:` annotation**, **trace matrix** | The comment that names the requirements a test covers. `cargo xtask trace` collects these annotations, the vector `requirements` lists, golden-trace headers, and the analysis records into a requirement → artefact matrix. | VER-020 |
| <a id="analysis-record"></a>**Analysis record**, **record** | A file in [`verification/records/`](verification/records/), named after the first requirement ID it covers (e.g. `records/VER-024.md`), that traces requirements verified by method A or I to the check or review that verifies them: a header table (Requirements, Method, Check) and free text. A requirement has at most one record, and there is no index. | [verification/analysis-records.md](verification/analysis-records.md) (format) |

#### Requirement areas

Areas CPP and APP belong to tracks C and A; the others are shared ([08 §6](08-roadmap.md#6-working-in-parallel) rule 7).

| Area | Covers | Document |
|---|---|---|
| <a id="area-gen"></a>**GEN** | Architecture, layering, and dependency rules; design rules: no heap, panic freedom, bounded execution, determinism, error model, bit conventions, configuration, `unsafe`, versioning | [01](01-architecture.md), [02](02-design-rules.md) |
| <a id="area-api"></a>**API** | The language-neutral API: identifier types, views and writers, strict and lenient decoding, configurations, the sans-I/O engine contract, buffers, security hooks, events, the error and field catalogues | [03](03-api-model.md) |
| <a id="area-lnk"></a>**LNK** | The link engines of `aldis-link`: packet generation, multiplexing, frame reception, packet extraction, segmentation and reassembly | [04](04-engines.md) |
| <a id="area-fw"></a>**FW** | The optional framework `aldis-framework`: traits, stacks, run loop, adapters, trace and replay | [05](05-framework.md) |
| <a id="area-ver"></a>**VER** | Verification: lints, panic freedom, testing, coverage, the target matrix, cross-checks, C and C++ checks, the verification run and merge gate | [06](06-verification.md) |
| <a id="area-tv"></a>**TV** | The conformance test-vector format and provenance | [07](07-test-vectors.md) |
| <a id="area-cmn"></a>**CMN** | Common link features: FECF CRC-16, OCF, OID pattern, frame counters, field placement with SDLS | [Common link features](standards/common-link-features.md) |
| <a id="area-time"></a>**TIME** | 301.0 time codes | [Time codes](standards/time-codes.md) |
| <a id="area-spp"></a>**SPP** | 133.0 Space Packet | [Space Packet](standards/space-packet.md) |
| <a id="area-epp"></a>**EPP** | 133.1 Encapsulation Packet | [Encapsulation Packet](standards/encapsulation-packet.md) |
| <a id="area-tm"></a>**TM** | 132.0 TM Transfer Frame | [TM frames](standards/tm-frame.md) |
| <a id="area-tc"></a>**TC** | 232.0 TC Transfer Frame, CLCW, frame delimiting and validation | [TC frames and CLCW](standards/tc-frame.md) |
| <a id="area-aos"></a>**AOS** | 732.0 AOS Transfer Frame, FHEC, M_PDU, B_PDU, editions | [AOS frames](standards/aos-frame.md) |
| <a id="area-uslp"></a>**USLP** | 732.1 USLP Transfer Frame, truncated frames, TFDF | [USLP frames](standards/uslp-frame.md) |
| <a id="area-cop"></a>**COP** | 232.1 COP-1: FOP-1 and FARM-1 | [COP-1](standards/cop1.md) |
| <a id="area-rs"></a>**RS** | The Rust profile: toolchain, crates and features, API shapes, lints, parse-don't-validate | [Rust profile](profiles/rust.md) |
| <a id="area-cabi"></a>**CABI** | The C ABI profile: linkage, naming, types, errors, memory model, versioning, rules for native C and C++ | [C ABI profile](profiles/c-abi.md) |
| <a id="area-cpp"></a>**CPP** | The native C++ implementation (track C): language subset, build, lints, verification, differential testing | [C++ profile](profiles/cpp.md) |
| <a id="area-app"></a>**APP** | The reference application scenario (track A) | [Reference application](reference-application.md) |

### 2.3 Architecture and API

| Term | Meaning | Defined in |
|---|---|---|
| <a id="layer"></a>**Layer 0–3** | The library's layers, each depending only on those below: **0** foundation (`aldis-core`); **1** stateless codecs (`aldis-time`, `-spp`, `-epp`, `-tm`, `-tc`, `-aos`, `-uslp`); **2** sans-I/O engines (`aldis-link`, `aldis-cop1`); **3** the optional framework (`aldis-framework`). The facade `aldis` and the C ABI crate `aldis-ffi` sit outside the layers. | [01 §1–2](01-architecture.md#1-layering); GEN-002 |
| <a id="codec"></a>**Codec** | A stateless function or type that encodes or decodes one CCSDS data unit (Layer 1). | [00 §4](00-introduction.md#4-terminology) |
| <a id="view"></a>**View** | The zero-copy, read-only result of a decode: typed accessors over the caller's buffer that cannot fail. | [00 §4](00-introduction.md#4-terminology); API-010 |
| <a id="frame-stream"></a>**Frame stream** | A zero-allocation iterator over a received data unit (such as a CLTU) that fuses delimiting, fill removal, and channel validation in a single pass, yielding accepted frame views or classified discard items. | TC-020, TC-021 |
| <a id="owned-header"></a>**Owned header** | A plain value copy of a view's header fields, with no borrow, for storing and comparing. | API-014 |
| <a id="writer"></a>**Writer**, **two-phase writer** | An encoder that writes a data unit into a caller-supplied buffer. A one-shot writer takes the header and payload; a two-phase writer first reserves the payload area for the caller to fill in place, then finishes the derived fields (lengths, FECF, FHEC). | [03 §2.2](03-api-model.md#22-writers-encoding); API-020…023 |
| <a id="engine"></a>**Engine**, **sans-I/O** | A deterministic protocol machine (Layer 2) that does no I/O itself: a pure step function over plain-data state. The caller feeds it an input and the current time and gets back the next state and a bounded list of actions. It has no output queue. | [03 §5](03-api-model.md#5-engines-the-sans-io-contract); GEN-030, API-057 |
| <a id="action"></a>**Action** | One item of the list an engine step returns: a frame to transmit, an SDU to deliver, or a notification, alert, or event. The list has a compile-time bound `N`. | [03 §5](03-api-model.md#5-engines-the-sans-io-contract); API-057 |
| <a id="framework"></a>**Framework** | The optional Layer 3 that drives the engines through transport and clock traits the integrator implements. | [05](05-framework.md) |
| <a id="facade"></a>**Facade** | The crate `aldis`, which re-exports the other crates behind one feature each. | [crates/aldis/README.md](../crates/aldis/README.md) |
| <a id="profile"></a>**Profile** | The binding of this language-neutral specification to one language: Rust, the C ABI, C++. | [00 §4](00-introduction.md#4-terminology) |
| <a id="instant"></a>**Instant** | Caller-supplied monotonic time in microseconds since an arbitrary epoch; engines never read a clock. | [00 §4](00-introduction.md#4-terminology); API-003 |
| <a id="managed-parameter"></a>**Managed parameter** | A CCSDS term: a parameter set by management rather than carried in the data, such as a frame length or whether the FECF is present. | [00 §4](00-introduction.md#4-terminology); §5 of each data link Blue Book |
| <a id="configuration"></a>**Configuration** | The immutable value type that holds a set of managed parameters (physical channel, master channel, VC, MAP, packet transfer). Its constructor rejects every combination the Blue Book forbids. | [03 §4](03-api-model.md#4-configuration-objects); GEN-060, GEN-061 |
| <a id="edition"></a>**Edition** | A configuration selector for a Blue Book issue whose wire format differs, such as AOS `Issue5` and `Issue4Compatible`. | GEN-062; AOS-001 |
| <a id="security-hook"></a>**Security hook**, **security layout** | The reservation for SDLS: a `SecurityLayout` fixes the Security Header and Trailer lengths per VC or MAP, and a `SecurityHook` (a helper in `aldis-core`, applied by the caller around an engine, never inside it) fills or checks them. v1 ships `NoSecurity` and a test-only `FixedPatternSecurity`. | [03 §7](03-api-model.md#7-security-sdls-applied-by-the-caller); API-070…075 |
| <a id="parse-dont-validate"></a>**Parse, don't validate** | The rule that a public type can hold only valid values: untrusted input is checked once, where it becomes the type, and code that receives the type does not check it again. | RS-020 |
| <a id="tooling-crate"></a>**Tooling crate** | A `std`-only crate that is never in a flight build (`aldis-testkit`, `xtask`, test and fuzz support), with the tooling lint profile. | RS-017 |

### 2.4 Decoding and errors

| Term | Meaning | Defined in |
|---|---|---|
| <a id="strict"></a>**Strict decoding** | The default mode: every violation of a Blue Book SHALL that is checkable from the octets and the configuration is an error. | API-030, API-031 |
| <a id="lenient"></a>**Lenient decoding** | The mode that accepts a short, fixed list of deviations (reserved and spare bits, fields declared undefined, conventional-zero fields, fixed-value fields of idle data units, and a few named others) and reports each one as an anomaly. | API-032; [03 §3](03-api-model.md#3-strict-and-lenient-decoding) |
| <a id="anomaly"></a>**Anomaly**, **anomaly flag** | A deviation that a lenient decode accepted, reported as a flag in an `AnomalySet` next to the successful result, never as an error. Each standard document lists its flags; a flag's bit index never changes once released. | GEN-043; API-033; e.g. [tm-frame.md](standards/tm-frame.md) §4 |
| <a id="discard-reason"></a>**Discard reason** | The standardized category of a frame or wire-octet rejection occurring during delimiting or frame validation checks (e.g. fill data, truncation, invalid channel, or CRC failure), reported next to discarded octets. | TC-021, TC-022 |
| <a id="error-kind"></a>**Error kind** | The category of a failure, with a stable numeric code shared by all profiles. | [03 §9](03-api-model.md#9-error-catalogue); GEN-040 |
| <a id="field-code"></a>**Field code** | The stable 16-bit number of a field or parameter that an error names, carried by the C ABI and compared by the differential tests; code 0 means "no field". | API-082; [03 §9.1](03-api-model.md#91-field-codes) |
| <a id="field-catalogue"></a>**Field catalogue** | The table in [03 §9.1](03-api-model.md#91-field-codes) that assigns the field codes. `cargo xtask fields --write` generates `aldis_core::field` and `test-vectors/core/fields.json` from it. | API-082 |

### 2.5 Test vectors

| Term | Meaning | Defined in |
|---|---|---|
| <a id="test-vector"></a>**Test vector** | A language-neutral JSON case in [`test-vectors/`](../test-vectors/README.md) with an input and the exact expected outcome. Every implementation must pass them: they are the contract between implementations. | [07](07-test-vectors.md) |
| <a id="provenance"></a>**Provenance** | Where a vector file's expected values come from: `blue-book-example` (an example printed in a Blue Book), `hand-derived` (worked from the Blue Book text, with a derivation note), `cross-checked` (agreed by a named independent implementation and version), or `fuzz-regression` (from a fuzzing crash). | TV-020 ([07 §4](07-test-vectors.md#4-provenance)) |
| <a id="released-vector"></a>**Released vector** | A vector case present in a release tag. It is never edited: a correction adds a new case. | TV-021; its check in [records/TV-021.md](verification/records/TV-021.md) |
| <a id="superseded-case"></a>**Superseded case**, **live case** | A released case marked `superseded` with a reason, and optionally the replacing case in `superseded.by`. Runners skip it; the cases they run are the live ones. | TV-021 |
| <a id="independent-model"></a>**Independent model**, **oracle** | A separate implementation that library results are compared with: a Python model written from the Blue Book before the Rust code (most hand-derived vectors; when it is a program it is committed under [`test-vectors/models/`](../test-vectors/models/README.md), TV-023), a model inside a fuzz target, or an `aldis_testkit::oracle` model such as `oracle::fhec`. An independent implementation used this way in a cross-check is called a test oracle. | [test-vectors/README.md](../test-vectors/README.md); [crates/aldis-testkit/README.md](../crates/aldis-testkit/README.md); [cross-checks.md](verification/cross-checks.md) §4 |
| <a id="cross-check"></a>**Cross-check** | Running the vectors against an existing implementation that shares no code or authors with Aldis, such as `spacepackets`, and resolving every disagreement from the Blue Book text. | VER-040; [cross-checks.md](verification/cross-checks.md) |
| <a id="differential-test"></a>**Differential test** | A fuzz or property test that runs the Rust crates and a native implementation (through its C ABI) on the same generated inputs and requires identical results. | [roadmap](08-roadmap.md) Phase 3 |
| <a id="exerciser"></a>**Exerciser** | A bare-metal binary in [`exerciser/`](../exerciser/) that calls every public API with the vector inputs, links on each bare-metal target, and runs the vectors under QEMU. | VER-010 |
| <a id="panic-freedom-link-check"></a>**Panic-freedom link check** | The exerciser's release build links a panic handler that calls an undefined symbol, `aldis_panic_is_unreachable`. The link succeeds only if no panic path survives optimization. | VER-010; GEN-021; [AGENTS.md](../AGENTS.md#toolchain-and-checks) |
| <a id="canary"></a>**Canary** | A build that deliberately adds one forbidden path (the Rust `inject-panic` feature plants an `unwrap()`) and must fail to link, which shows the check works. | [M0 plan](milestones/m0-foundations.md) WP-5 |
| <a id="verification-run"></a>**Verification run** | `cargo xtask verify`: every gating check the change needs, run on one clean commit in pinned containers. Each check declares its inputs and is either portable or bound to a host. | VER-056 ([06 §7](06-verification.md#7-verification-runs-and-the-merge-gate)) |
| <a id="evidence-record"></a>**Evidence record** | The canonical JSON that a verification run writes: commit and tree, host, images and tool versions, the scope, and each check's result and log hash. `--publish` stores it as a git note under `refs/notes/verify`. | VER-057 |
| <a id="merge-gate"></a>**Merge gate** | A pull request merges when its head contains the tip of `main` and has a published record of that tree in which every portable check passed, was skipped, or was reused; the commit status `aldis/verify` reports it. | VER-058 |
| <a id="skipped-check"></a>**Skipped check**, **reused check** | A check left out of a run and its result taken from an earlier published record whose inputs are identical: *skipped* when none of its inputs differs from the merge base either, *reused* when its inputs differ from the base but match an earlier verified commit of the branch. Documentation is an input of the `spec` check only, so a documentation-only change runs only `spec`. | VER-056 |
| <a id="nightly"></a>**Nightly** | (1) The GitHub Actions runs each night on `main`, which repeat every check and add the night-only ones (fuzzing, mutation testing, stack usage). (2) The *tooling nightly*: the pinned nightly Rust toolchain used only for tooling such as coverage and fuzzing. | (1) VER-059; (2) RS-002, OI-14 |
| <a id="coverage-waiver"></a>**Coverage waiver** | A justified exception to the 100 % coverage gate, listed in `coverage-waivers.toml` and reviewed per release. | VER-024 |
| <a id="mutation-triage"></a>**Mutants triage** | The record of every mutant that survived mutation testing and why. | VER-025; [mutants-triage.md](verification/mutants-triage.md) |

### 2.7 Reference application

| Term | Meaning | Defined in |
|---|---|---|
| <a id="reference-application"></a>**Reference application** | The end-to-end flight and ground application in `refapp/` (track A), built on the public APIs only. | [reference-application.md](reference-application.md) |
| <a id="node"></a>**Node**, **harness** | The flight and ground nodes are sans-I/O applications; the harness owns time, the simulated link, faults, and the trace. | [reference-application.md](reference-application.md) §2, §6, §8 |
| <a id="golden-trace"></a>**Golden trace** | The canonical JSON Lines record of every data unit exchanged in one scenario case. Every implementation must reproduce it byte for byte. Golden traces are regression baselines, not conformance vectors, and each change bumps their version. | [reference-application.md](reference-application.md) §7; APP-080…088; OI-23 |
| <a id="hk"></a>**HK** | Housekeeping: the spacecraft's periodic status telemetry, one of the scenario's packet streams. | [reference-application.md](reference-application.md) §3.1 |

## 3. Blue Book documents

[`specs/README.md`](../specs/README.md) explains how to download the documents in [`specs/manifest.tsv`](../specs/manifest.tsv). Group *v1* holds the standards implemented in v1, group *ref* other documents the specification cites. Titles of documents that are not in the manifest are given as the Blue Book named in the last column cites them.

| Document | Title | In `manifest.tsv` | Specified in Aldis by |
|---|---|---|---|
| <a id="ccsds-132-0-b-3"></a>132.0-B-3 | TM Space Data Link Protocol | Yes (v1) | [TM frames](standards/tm-frame.md) |
| <a id="ccsds-232-0-b-4"></a>232.0-B-4, with Cor. 1 | TC Space Data Link Protocol; Technical Corrigendum 1 to CCSDS 232.0-B-4 | Yes (v1), both | [TC frames and CLCW](standards/tc-frame.md) |
| <a id="ccsds-232-1-b-2"></a>232.1-B-2, with Cor. 1 | Communications Operation Procedure-1; Technical Corrigendum 1 to CCSDS 232.1-B-2 | Yes (v1), both | [COP-1](standards/cop1.md) |
| <a id="ccsds-732-0-b-5"></a>732.0-B-5 | AOS Space Data Link Protocol | Yes (v1) | [AOS frames](standards/aos-frame.md) |
| <a id="ccsds-732-0-b-4"></a>732.0-B-4 (Silver Book) | AOS Space Data Link Protocol, Issue 4: superseded, kept for `Issue4Compatible` | Yes (ref) | AOS-002, AOS-003, AOS-013; OI-04 |
| <a id="ccsds-732-1-b-3"></a>732.1-B-3 | Unified Space Data Link Protocol | Yes (v1) | [USLP frames](standards/uslp-frame.md) |
| <a id="ccsds-133-0-b-2"></a>133.0-B-2 | Space Packet Protocol | Yes (v1) | [Space Packet](standards/space-packet.md) |
| <a id="ccsds-133-1-b-3"></a>133.1-B-3 | Encapsulation Packet Protocol | Yes (v1) | [Encapsulation Packet](standards/encapsulation-packet.md) |
| <a id="ccsds-301-0-b-4"></a>301.0-B-4 | Time Code Formats | Yes (v1) | [Time codes](standards/time-codes.md) |
| <a id="ccsds-131-0-b-6"></a>131.0-B-6 | TM Synchronization and Channel Coding | Yes (ref) | Out of v1 scope; roadmap §7 |
| <a id="ccsds-231-0-b-4"></a>231.0-B-4 | TC Synchronization and Channel Coding | Yes (ref) | Out of v1 scope; roadmap §7; OI-35 item 8 |
| <a id="ccsds-355-0-b-2"></a>355.0-B-2 | Space Data Link Security Protocol | Yes (ref) | SDLS hooks: API-070…075 |
| <a id="ccsds-355-1-b-1"></a>355.1-B-1 | Space Data Link Security Protocol — Extended Procedures (as cited by 132.0 ref. [11] and 732.1 ref. [16]) | No | Defines the FSR; SDLS is roadmap v2 |
| <a id="ccsds-700-1-g-1"></a>700.1-G-1 (Green Book) | Overview of the Unified Space Data Link Protocol: informational, written for 732.1-B-1, never overrides the Blue Book | Yes (ref) | OI-05; OI-35 item 8 |
| <a id="ccsds-211-0-b-6"></a>211.0-B-6 | Proximity-1 Space Link Protocol — Data Link Layer (as cited by 732.1 ref. [10]) | No | Defines COP-P and the PLCW; roadmap v3 |
| <a id="ccsds-320-0-m-7"></a>320.0-M-7 (Magenta Book) | CCSDS Spacecraft Identification Field Code Assignment Control Procedures (as cited by 132.0 ref. [7]); [00 §6](00-introduction.md#6-references) cites it as 320.0-M | No | SCID assignment; OI-26 |
| <a id="ccsds-352-0-b-2"></a>352.0-B-2 | CCSDS Cryptographic Algorithms (as cited by 355.0 ref. [4]); the roadmap cites it as 352.0 | No | Roadmap v2 |
| <a id="ccsds-727-0-b-4"></a>727.0-B-4 | CCSDS File Delivery Protocol (CFDP) (as cited by 133.0 ref. [C10]); the roadmap cites it as 727.0 | No | Roadmap v2 |

The roadmap ([08 §7](08-roadmap.md#7-later-standards-proposed-order)) also names later standards by number only (211.x, 734.x, 702.1, 502–509, 660.0, 911–922, 913.1); they are not listed here.
