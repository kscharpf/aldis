# 05 — Framework (`aldis-framework`)

The framework is **optional**. It turns Layer 2 engines into running protocol stacks by driving them through small, heapless traits that the integrator implements. It adds no protocol behavior of its own: everything protocol-related stays in the engines, so library users and framework users get identical wire behavior.

## 1. Traits

```
trait Clock      { fn now(&self) -> Instant; }

trait FrameSink  {                       // downward: to radio / modem / SLE / socket
    fn ready(&self) -> bool;             // can accept one more frame now?
    fn send(&mut self, frame: &[u8], meta: &TxMeta) -> Result<(), TransportError>;
}

trait FrameSource {                      // upward: from radio / modem / SLE / socket
    fn recv(&mut self, buf: &mut [u8]) -> Result<Option<(usize, RxMeta)>, TransportError>;  // non-blocking
}

trait SduSink    { fn deliver(&mut self, sdu: &[u8], meta: &SduMeta) -> Result<(), TransportError>; }
trait EventSink  { fn event(&mut self, ev: &Event); }
```

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| FW-001 | All traits SHALL be usable in `no_std` without `alloc`. No trait method SHALL require blocking. Implementations return "not ready" instead. | — | A |
| FW-002 | The framework SHALL NOT spawn threads or tasks, nor depend on an async runtime, in its default configuration. An `async` feature MAY add `async fn` variants of the traits and the run loop. | — | A |
| FW-003 | `TransportError` SHALL be reported to the `EventSink` and SHALL NOT corrupt engine state. A frame that fails to send is treated as a Lower-Procedure *reject* where COP-1 applies (E42/E44/E46). | 232.1 E41–E46 | T |

## 2. Stacks and run loop

The framework provides pre-wired **stacks**, built from the engines of [04](04-engines.md) and [COP-1](standards/cop1.md):

| Stack | Composition |
|---|---|
| `TmDownlinkStack` | PacketGenerators + ChannelMux (TM), optional FARM-1 CLCW → OCF wiring |
| `AosDownlinkStack` | as above with AOS, Insert Zone, Bitstream |
| `UslpLinkStack` | USLP send/receive, MAP segmenter/reassembler, optional COP-1 |
| `TcUplinkReceiverStack` | FrameDelimiter + FrameReceiver + FARM-1 per VC + MapReassembler (flight side) |
| `TcUplinkSenderStack` | MapSegmenter + FOP-1 per VC + CLTU packer; consumes CLCWs from a downlink receiver (ground side) |
| `TelemetryReceiverStack` | FrameReceiver + PacketExtractors; routes CLCWs to a `TcUplinkSenderStack` (ground side) |

```
loop step(now):
    pull frames from FrameSource      → engine.handle_frame(now, …)
    drain engine outputs              → FrameSink.send / SduSink.deliver / EventSink.event
    acknowledge sends (COP-1 Out flags) → engine.transmit_done(…)
    if now ≥ min(poll_timeout)        → engine.handle_timeout(now)
    return next deadline              → caller sleeps / waits for interrupt until then
```

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| FW-010 | `Stack::step(now) -> Option<Instant>` SHALL perform one bounded pass as above and return the next deadline. It SHALL be callable from a cyclic executive, an RTOS task, an interrupt-deferred handler, or a host event loop. | — | T |
| FW-011 | `step` SHALL process at most a configured number of frames per call, to bound its execution time (GEN-023). | — | T |
| FW-012 | Stacks SHALL expose their constituent engines for direct access (status snapshots, directives), so applications can mix framework and library usage. | — | I |
| FW-013 | A stack's behavior SHALL be reproducible from a recorded trace of `(now, FrameSource input, FrameSink readiness)`. The framework SHALL provide a trace recorder and a replayer behind the `trace` feature. | GEN-030 | T |

## 3. Reference adapters

| Adapter | Feature | Notes |
|---|---|---|
| `UdpFrameTransport` (std) | `std-udp` | One frame per datagram; for ground testing, simulators, and gateways |
| `TcpFrameTransport` (std) | `std-tcp` | Length-prefixed frames on a stream |
| `EmbeddedNalTransport` | `embedded-nal` | Heapless no_std sockets via the `embedded-nal` traits, for flight computers with an IP stack |
| `LoopbackLink` | default | In-memory link with loss/delay/duplication injection, used by the COP-1 closed-loop tests (COP-040) |
| `StdClock` | `std` | Monotonic clock from `std::time::Instant` |

| ID | Requirement | Source | Verif. |
|---|---|---|---|
| FW-020 | Each adapter SHALL be gated by its own feature and SHALL NOT be compiled by default except `LoopbackLink`. | GEN-004 | A |
| FW-021 | Socket-based adapters SHALL be documented as **test and ground integration tools**. Framing on sockets (length prefix, one frame per datagram) is a library convention, not a CCSDS standard. | — | I |
