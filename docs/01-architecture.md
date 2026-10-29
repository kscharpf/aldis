# 01 — Architecture

## 1. Layering

The libraries are split into three layers plus a foreign-function boundary. Each layer depends only on the layers below it.

```
 ┌────────────────────────────────────────────────────────────────────────┐
 │ Layer 3 — FRAMEWORK (optional)                         aldis-framework │
 │   Transport/clock/timer traits, run loop, reference adapters           │
 │   ("framework behavior": you plug in I/O, it drives the engines)       │
 ├────────────────────────────────────────────────────────────────────────┤
 │ Layer 2 — ENGINES (sans-I/O, stateful, deterministic)                  │
 │   aldis-cop1 : FOP-1, FARM-1                                           │
 │   aldis-link : packet→frame generation, MC/VC multiplexing, OID,       │
 │                frame demultiplexing, packet extraction, TC/USLP        │
 │                segmentation & reassembly, frame-count tracking         │
 ├────────────────────────────────────────────────────────────────────────┤
 │ Layer 1 — CODECS (stateless, bytes in → fields/bytes out)              │
 │   aldis-time  aldis-spp  aldis-epp  aldis-tm  aldis-tc  aldis-aos      │
 │   aldis-uslp                                                           │
 ├────────────────────────────────────────────────────────────────────────┤
 │ Layer 0 — FOUNDATION                                       aldis-core  │
 │   errors, bit-field access, IDs, CRC-16, OID PN generator,             │
 │   fixed-capacity containers, Instant/Duration, security-hook traits    │
 └────────────────────────────────────────────────────────────────────────┘
   aldis      : facade crate re-exporting the above behind feature flags
   aldis-ffi  : C ABI over layers 0–2 (and 3 where meaningful)
```

### 1.1 Library vs framework behavior

Each protocol behavior is available in two ways:

- **Library behavior (Layers 1–2).** The application keeps control. It calls codecs, feeds inputs and the current time into engines, and takes the returned actions (frames to send, SDUs, events) and the next timer deadline back out. Nothing happens unless the application calls in. This is the mode for flight software, which has its own scheduler, and for integrating into existing ground systems.
- **Framework behavior (Layer 3).** The application provides implementations of small transport and clock traits. The framework's run loop pumps the engines, sends their output, arms timers, and dispatches events to handlers. Retransmission (COP-1), frame release timing, and OID insertion then just happen. It is still `no_std` and heapless, but it owns the control flow.

Retransmission logic (FOP-1/FARM-1) lives in its own crate, `aldis-cop1`, at Layer 2. It is therefore usable as a plain library (`fop.step(Input::Clcw(..), now, out)` returns the next state and the frames to transmit, API-057) and also as part of the framework, without code duplication.

## 2. Crate map

| Crate | Layer | Implements | Depends on |
|---|---|---|---|
| `aldis-core` | 0 | Shared types and algorithms ([common link features](standards/common-link-features.md)) | — |
| `aldis-time` | 1 | 301.0 time codes | core |
| `aldis-spp` | 1 | 133.0 Space Packet | core, (time: optional feature for typed secondary header) |
| `aldis-epp` | 1 | 133.1 Encapsulation Packet | core |
| `aldis-tm` | 1 | 132.0 TM Transfer Frame | core |
| `aldis-tc` | 1 | 232.0 TC Transfer Frame, Segment Header, Control Commands, CLCW, frame delimiting | core |
| `aldis-aos` | 1 | 732.0 AOS Transfer Frame, FHEC, M_PDU, B_PDU | core |
| `aldis-uslp` | 1 | 732.1 USLP Transfer Frame (incl. truncated), TFDF header | core |
| `aldis-link` | 2 | Sending and receiving engines for packets over TM/AOS/USLP and TC/USLP MAPs ([04](04-engines.md)) | core, spp, epp, tm, tc, aos, uslp (each behind a feature) |
| `aldis-cop1` | 2 | 232.1 FOP-1, FARM-1 ([COP-1](standards/cop1.md)) | core, tc, (uslp feature) |
| `aldis-framework` | 3 | [05 Framework](05-framework.md) | core, link, cop1 |
| `aldis` | facade | Re-exports | all, by feature |
| `aldis-ffi` | boundary | [C ABI profile](profiles/c-abi.md) | aldis (selected features) |

### 2.1 Dependency rules

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| GEN-001 | Crates in Layers 0–2 SHALL have **no third-party runtime dependencies**. Dev-dependencies (test, fuzz, property tooling) are permitted. | — | A |
| GEN-002 | A crate SHALL NOT depend on a crate in the same or a higher layer, except the Layer 1 → Layer 1 optional dependency `aldis-spp → aldis-time`. | — | A |
| GEN-003 | Every Layer 1 codec crate SHALL be independently usable. Selecting only `aldis-spp` SHALL NOT compile any frame-protocol code. | — | A |
| GEN-004 | Layer 3 MAY have optional third-party dependencies, and only behind non-default features (e.g. `embedded-nal` adapters, `std` socket adapters). | — | A |

The goal of GEN-001 to GEN-003 is to **keep the certification surface minimal**. A mission that only builds Space Packets qualifies only `aldis-core` and `aldis-spp`.

## 3. Data flows

### 3.1 Spacecraft downlink (flight)

```
 applications ──packets──▶ [aldis-spp encode]
                                  │
                                  ▼
              aldis-link: VC packet generator (per VC)   ◀── CLCW from FARM-1 (OCF source)
                                  │  frame data fields + FHP
                                  ▼
              aldis-link: MC / physical channel multiplexer
                 · frame counts · FSH/OCF insertion · OID frames · FECF
                                  │  complete TM / AOS / USLP frames (bytes)
                                  ▼
                     integrator: channel coding + radio   (outside v1)
```

### 3.2 Spacecraft uplink (flight)

```
   radio + CLTU decoding (outside v1) ──decoded octets──▶ [aldis-tc frame delimiting & validation]
                                                                  │ valid TC / USLP frames
                                                                  ▼
                                                   aldis-cop1: FARM-1 (per VC) ──▶ CLCW ──▶ downlink OCF
                                                                  │ accepted FDUs
                                                                  ▼
                                         aldis-link: MAP demux + segment reassembly
                                                                  │ packets / MAP_SDUs
                                                                  ▼
                                                            applications
```

### 3.3 Ground (mirror image)

- **Commanding:** packets → `aldis-link` MAP segmentation/blocking → `aldis-cop1` FOP-1 (AD/BD/BC) → TC/USLP frames → integrator's CLTU encoding / SLE FCLTU. CLCWs extracted from the downlink OCF are fed back into FOP-1.
- **Telemetry:** frames from the front end or SLE RAF → `aldis-link` frame demultiplexing (validation, frame-count gaps, OCF/FSH extraction) → packet extraction → packets → `aldis-spp` decode (+ `aldis-time` for time stamps).

## 4. Build configurations

| Configuration | Features | Intended targets |
|---|---|---|
| **Bare metal (default)** | none: `no_std`, no `alloc` | Cortex-M/R, RISC-V, LEON (where Rust supports it) |
| **RTOS with heap** | `alloc` | Adds owned/growable convenience types only. Engines remain fixed-capacity. |
| **Hosted** | `std` (implies `alloc`) | Linux/macOS/Windows ground systems; adds `std::error::Error` impls, reference transport adapters (framework) |

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| GEN-005 | All crates SHALL build with **no features enabled** for a `no_std` target with no global allocator. | — | A |
| GEN-006 | The `alloc` and `std` features SHALL be strictly additive. Enabling them SHALL NOT change the behavior or wire output of any existing API. | — | T/V |
| GEN-007 | Behavior SHALL be identical across targets of different endianness and pointer width. | — | V |

## 5. Relationship to native C/C++ implementations (Phase 3)

Native C and C++ implementations are planned in Phase 3 of the [roadmap](08-roadmap.md) (Track C native C++ was retired from v1 and deferred to Phase 3). When implemented, they mirror this layering module for module and expose the same C ABI and C++ wrapper API (`aldis.hpp`).
