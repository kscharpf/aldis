# 04 — Engines (`aldis-link`)

`aldis-link` implements the **protocol procedures** of the data link standards (the "Protocol Procedures at the Sending/Receiving End" sections of 132.0, 232.0, 732.0, and 732.1) as sans-I/O engines following the contract in [03 §5](03-api-model.md#5-engines-the-sans-io-contract). COP-1 is specified separately in [standards/cop1.md](standards/cop1.md).

Engines are composable building blocks. A typical deployment instantiates one engine per VC (or MAP) plus one multiplexer per physical channel.

```
SENDING                                               RECEIVING
packets ─▶ PacketGenerator (per VC) ─┐                ┌─▶ FrameReceiver (per physical channel)
VCA SDUs ─▶ AccessGenerator (per VC) ─┼▶ ChannelMux ─▶ │     validate · FECF · demux · counters · OCF/FSH/IZ
B-stream ─▶ BitstreamGenerator (AOS) ┘   (per PC)     │   ├─▶ PacketExtractor (per VC)  ─▶ packets
OCF source, FSH source, IN_SDU ─────────▶             │   ├─▶ AccessReceiver (per VC)   ─▶ VCA SDUs
                                                      │   └─▶ MapReassembler (TC/USLP)  ─▶ packets / MAP SDUs
MAP SDUs/packets ─▶ MapSegmenter (TC/USLP) ─▶ FOP-1 / direct BD ─▶ frames
```

## 1. Common engine requirements

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| LNK-001 | Every engine SHALL be generic over (or configured for) exactly one frame protocol: TM, TC, AOS, or USLP. Protocol differences SHALL be handled by protocol-specific configuration, not runtime type checks of frames. | — | I |
| LNK-002 | Every engine SHALL expose counters for housekeeping: frames/SDUs in and out, idle generated, discards by reason, gaps, and queue high-water marks (API-056). | — | T |
| LNK-003 | `reconfigure(config)` SHALL be permitted only between data units. Partially assembled data (a packet spanning frames, a segmented SDU) SHALL be either flushed per the engine's documented policy or discarded with an event. | GEN-061 | T |

## 2. Sending side

### 2.1 PacketGenerator (TM / AOS / USLP fixed-length; VC Packet and MAP Packet services)

Fills fixed-length frame data fields (TM data field, AOS M_PDU packet zone, USLP rule `000` TFDZ) with packets, inserted contiguously and in forward order, spanning frames as needed, and computes the FHP.

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| LNK-010 | `submit_packet(now, octets)` SHALL accept a packet only if its PVN is in the configured valid set and its length ≤ the configured maximum. It SHALL be delimitable by `peek_length` (SPP or EPP). Otherwise it SHALL return `InvalidField`/`OutOfRange`. | 132.0 §3.3; Table 5-4 | T |
| LNK-011 | Packets SHALL be placed contiguously and in submission order. The FHP of each data field SHALL point to the first packet *starting* in it, or SHALL be the protocol's "no packet starts" value. | 132.0 §4.1.2.7.6, §4.1.4.5; 732.0 §4.1.4.2.2; 732.1 §4.1.4.2.4.3–4 | V |
| LNK-012 | A data field SHALL be released when it is full, or when the configured **release deadline** expires after its first octet was written (USLP: "maximum delay … for a TFDF to be completed", Table 5-3; TM/AOS: equivalent mission parameter). The generator SHALL report the deadline via `next_deadline` (API-053) and SHALL release the field on the `Timeout` input. | 732.1 Table 5-3; 132.0 §4.2 | V |
| LNK-013 | On deadline release of a partly filled data field, the remainder SHALL be completed with an **idle packet**. TM/AOS: a Space Packet Idle Packet (APID 2047), or an Encapsulation Idle Packet if EPP is a valid PVN on the VC. If the remainder is smaller than the minimum idle packet length (7 for SPP), the idle packet SHALL continue into the next data field. USLP: an Encapsulation Idle Packet, which can be as short as 1 octet, so it always fits. | 132.0 §4.1.4.6; 732.0 §4.1.4.2.3.4; 732.1 §4.1.4.3.4 | V |
| LNK-014 | When no packet data is available at frame release time and the VC carries packets, the generator SHALL produce an idle-only data field (TM: handled by the ChannelMux as OID or idle-packet frame per policy; AOS: M_PDU with FHP = "only idle data"). | 132.0 §4.1.4.6; 732.0 §4.1.4.2.3.4 | V |
| LNK-015 | Packet queue capacity (octets) SHALL be compile-time. `submit_packet` SHALL return `WouldBlock` when full (API-052). | — | T |

### 2.2 AccessGenerator and BitstreamGenerator

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| LNK-020 | VC Access (TM/AOS) SHALL accept SDUs of exactly the data field length. USLP VCA/MAPA over fixed-length frames SHALL use rules `001`/`010` with the Last Valid Octet Pointer. | 132.0 §3.4; 732.0 §3.5; 732.1 §4.1.4.2.2.2.2–3 | V |
| LNK-021 | The AOS BitstreamGenerator SHALL fill B_PDUs from a bit queue and set the Bitstream Data Pointer to the last valid bit, or to "all valid" or "only idle". | 732.0 §4.1.4.3 | V |

### 2.3 ChannelMux (per physical channel; TM, AOS, fixed-length USLP)

Multiplexes VCs into master channels and master channels into the physical channel. It owns the MC and VC frame counters, inserts FSH/OCF/Insert Zone, and generates OID frames. It returns each frame as a `Transmit` action ready for the caller's SDLS processing, with the security ranges; the caller applies SDLS and the codec writer computes the FHEC and the FECF (API-074).

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| LNK-030 | The input `TransmitOpportunity` SHALL always return a `Transmit` action with the next frame for a synchronous physical channel. If no VC has a ready data field, it SHALL produce an **OID frame** (TM: on a configured packet VC; AOS/USLP: VCID 63). | 132.0 §4.1.4.6; 732.0 §4.1.4.1.5; 732.1 §4.1.4.1.5–7 | V |
| LNK-031 | VC selection SHALL be delegated to a `MuxPolicy`. The library SHALL provide `FixedPriority`, `RoundRobin`, and `WeightedRoundRobin` policies. Custom policies SHALL be pure functions of mux state (GEN-030). | 132.0 §4.2 (mission-specific scheme) | T |
| LNK-032 | TM MC frame count SHALL increment on every frame of the master channel, including OID frames. VC frame counts SHALL increment per VC (AOS: 24-bit, with the cycle counter when enabled). | 132.0 §4.1.2.5–6; 732.0 §4.1.2.4–5 | V |
| LNK-033 | MC_OCF and VC_OCF content SHALL come from an `OcfSource` slot that the caller updates (e.g. with the latest FARM-1 CLCW). The mux SHALL insert the slot's current value into every frame where the OCF is configured. | 132.0 §4.1.5; 232.0 §4.2.1.1.1 note 2 | V |
| LNK-034 | Insert Zone content (AOS/USLP) SHALL come from an `InsertSource` slot. The newest IN_SDU SHALL be inserted into every frame, including OID frames. If no new IN_SDU has arrived since the last frame, the mux SHALL insert the configured fill and raise an `InSduLoss` indication (USLP: IN_SDU Loss Flag). | 732.0 §4.1.3; 732.1 §3.11.2.4, §4.1.3 | V |
| LNK-035 | Each frame SHALL be finalized in this order: all header and data fields (by the mux, which returns them with the security ranges) → `SecurityHook::protect` (by the caller) → FHEC (AOS) → FECF (by the codec writer, API-074; RS-023 makes the order a typestate). | 355.0 §2.3 | T |
| LNK-036 | The OID generator state SHALL persist across OID frames for the life of the mux (CMN-021). | 132.0 §4.1.4.6.2 | V |

### 2.4 MapSegmenter (TC; USLP variable-length)

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| LNK-040 | For TC VCs with Segment Headers, SDUs larger than the configured maximum FDU SHALL be segmented with sequence flags `01`/`00`/`10`, and complete SDUs sent as `11`. Blocking of several complete packets into one FDU SHALL be applied only where the MAP or VC permits it. | 232.0 §4.1.3.2.2, §4.3; Table 5-3/5-4 | V |
| LNK-041 | Once segmentation of an SDU has started on a VC, no SDU of another MAP SHALL be interleaved on that VC until the SDU completes. | 232.0 §4.3; 732.1 §4.1.2.5.3 | V |
| LNK-042 | For USLP variable-length frames, the segmenter SHALL use rules `111` (unsegmented), `100`/`101`/`110` (start/continue/last), or `011` (octet stream). Continuation segments SHALL be sent on the same GMAP ID with contiguous VCF counts. | 732.1 §4.1.4.2.2 | V |
| LNK-043 | The segmenter output SHALL be FDUs/TFDFs handed to FOP-1 (AD) or emitted directly as BD frames. The QoS SHALL be chosen per request. | 232.0 §3; 732.1 §3 | T |
| LNK-044 | A TC/USLP **CLTU packer** utility SHALL group consecutive frames for one C&S data unit up to the configured maximum number of frames and octets. It SHALL never mix truncated and non-truncated USLP frames. | 232.0 Table 5-1; 732.1 Table 5-1, D1.2.5 | T |

## 3. Receiving side

### 3.1 FrameReceiver (per physical channel)

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| LNK-050 | The input `Frame(octets)` SHALL be a frame that the caller has decoded with the protocol codec, whose FECF it has checked (correcting the AOS FHEC), and whose SDLS processing it has verified (`SecurityHook::verify`, API-074; RS-023 makes this a typestate). The engine SHALL validate SCID/VCID/flags against configuration and route the frame to its VC. Each frame the engine discards SHALL return a `FrameDiscarded{reason}` action. A frame the caller discards before the engine (FECF, FHEC, SDLS) is counted and reported with the same event by the caller or the framework stage. | 132.0 §4.3; 232.0 §4.4.9; 732.0 §4.3; 732.1 §4.3 | V |
| LNK-051 | The receiver SHALL track MC and VC frame counts (CMN-030) and raise `FrameGap{vc, missing}` on discontinuity. A gap SHALL be propagated to the VC's extractor/reassembler so that it discards partial data. | 132.0 §4.1.2.6 note; 732.1 §4.1.2.12 note 3 | V |
| LNK-052 | OID frames SHALL be counted and their data discarded. Their Insert Zone, OCF, and FSH SHALL still be delivered. | 132.0 §4.1.4.6 note 1; 732.0 §4.1.3.3 | V |
| LNK-053 | OCF contents SHALL be emitted as `Event::Ocf{vc, report}`. CLCWs SHALL be decodable for direct feed into FOP-1. FSH and Insert Zone contents SHALL be emitted as events or delivered SDUs per configuration. | 132.0 §3.5–3.9; 732.0 §3.6, §3.9 | V |
| LNK-054 | The AOS Replay Flag SHALL be passed through as metadata on every delivered SDU. | 732.0 §4.1.2.5.2 | T |

### 3.2 PacketExtractor (per VC)

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| LNK-060 | The extractor SHALL reassemble packets from consecutive data fields using the FHP and each packet's length field (SPP or EPP by PVN). It SHALL deliver each complete packet once. | 132.0 §4.3 (packet extraction); 732.0 §4.3; 732.1 §4.3 | V |
| LNK-061 | After a frame gap or when starting up, the extractor SHALL discard the partial packet and resynchronize at the next FHP. If "deliver incomplete packets" is configured, it SHALL instead deliver the partial packet flagged `Incomplete`. | Table 5-4 (deliver incomplete) | V |
| LNK-062 | A packet whose length field exceeds the configured maximum, or whose PVN is not valid, SHALL cause the extractor to discard data until the next FHP and raise `ExtractionError`. | 132.0 Table 5-4 | V/F |
| LNK-063 | Idle packets (SPP APID 2047, EPP EPI `000`) SHALL be dropped by default, with a counter; delivery MAY be enabled for diagnostics. | 133.0 §4.1.3.3.4.4; 133.1 §4.1.2.3 | V |
| LNK-064 | The extractor SHALL ignore the data field of frames whose FHP indicates "only idle data". | 132.0 §4.1.2.7.6.5; 732.0 §4.1.4.2.2.5 | V |

### 3.3 MapReassembler (TC Segment Header; USLP rules)

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| LNK-070 | The reassembler SHALL rebuild MAP SDUs from segments in order, per MAP. A segment sequence violation (e.g. a continuation without a first segment) SHALL discard the partial SDU and raise an event. | 232.0 §4.4.3; 732.1 §4.3 | V |
| LNK-071 | Blocked FDUs containing several packets SHALL be split into individual packets. | 232.0 §4.1.3.2.1.2–3 | V |
| LNK-072 | For USLP fixed-length MAPA/VCA (rules `001`/`010`), the reassembler SHALL use the Last Valid Octet Pointer to delimit SDUs and discard idle octets. | 732.1 §4.1.4.2.4.5–6 | V |
| LNK-073 | The maximum SDU size SHALL be a compile-time capacity, checked against the configured `max MAP_SDU length`. | 232.0 Table 5-4 | T |
