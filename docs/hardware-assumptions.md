# Hardware assumptions

What Aldis assumes of the platform it runs on, why, and what happens when an assumption does not hold. An integrator SHALL read this before using Aldis on hardware exposed to radiation (GEN-015).

## 1. Memory holding Aldis state is protected against single-event upsets

**Assumption (GEN-015).** Memory that holds Aldis state *between* calls is protected against single-event upsets (SEUs) by the platform, or holds data that does not change at run time. The state that lives across calls is:
- **configurations**: physical, master, and virtual channel tables, and MAP tables;
- **engine state**, from Milestone 3: FOP-1 and FARM-1 counters, windows, and protocol states, and later the link engines;
- **helper storage** in caller memory, such as the segmenter and reassembler, and the C ABI handles.

"Protected" means error-correcting memory (EDAC/ECC) with scrubbing, or an equivalent the platform provides. Single-bit errors are then corrected before software reads them, and uncorrectable errors are reported to the platform (a trap or a reset) rather than silently read. Radiation-tolerant processors commonly provide this. An example is the fault-tolerant LEON parts, for which Aldis provides best-effort Rust support (OI-08). The integrator confirms it for the actual part and memory map.

Data received on a link is **not** covered by this assumption, and needs no such protection. Bit errors in frames on the link are the job of the frame checks (FECF, FHEC; TM-006, TC-006, AOS-013/014, USLP's FECF), and Aldis rejects a corrupted frame with an error.

## 2. Why: what an upset in unprotected memory does to Aldis

Aldis is written so that **invalid values cannot be constructed** (RS-020, "parse, don't validate") and **invalid states cannot be represented** (RS-021). Untrusted input is checked once, where it becomes a type, and code that holds the type relies on it. That rules out a whole class of logic errors. But it assumes the value, once checked, does not change on its own. An SEU in unprotected memory breaks exactly that assumption:

1. **A corrupted enum is undefined behaviour.** Rust assumes an enum's discriminant always names one of its variants. That covers protocol states, frame types, and pointer kinds. A bit flip that gives a discriminant naming no variant cannot be detected by safe code: reading it is undefined behaviour. The compiler is entitled to have removed the branches it proved unreachable, so a `match` on a corrupted state may take any path, not an error path. A C `switch` with a `default:` arm can catch a corrupted value; Rust cannot express that check, because the value cannot legally exist.
2. **A corrupted restricted value is not checked again.** A `PacketOffset`, a `VcfCountLen`, or an identifier whose stored bits change after construction stays out of range for the rest of its life. RS-020 deliberately does not re-check it.
3. **Memory safety still holds where it matters most.** Every slice access is bounds-checked (GEN-020): Aldis has no unchecked indexing, and no `unsafe` outside `aldis-ffi`. A corrupted length or offset leads to a wrong result or an error, not to a read or write outside the buffer. Undefined behaviour from a corrupted enum is not bounded this way.
4. **Exposure is small for the codecs and grows with long-lived state.**
   - **Codecs:** stateless. Each call parses from raw octets, and the parsed values live on the stack only for that call.
   - **Configurations:** live for the whole mission. Their constructors are `const` (RS-012), so they can be placed in flash or ROM (§3).
   - **Engine state (M3 onward):** lives in RAM for the whole mission and changes on every call. This is where an unprotected upset is most likely to matter. M3 planning decides how Aldis hardens it (roadmap, M3).

**Without the assumption, no language is safe.** A corrupted pointer, return address, or loop counter in C or C++ is as dangerous as a corrupted enum in Rust; it is only harder to reason about. Aldis does not try to make software alone tolerate arbitrary memory corruption. It states the assumption so that the integrator can meet it.

## 3. What the integrator does

- **Run Aldis on memory with EDAC and scrubbing** for its state, and confirm it for the part and the memory map. This covers the stack, the static data holding configurations, and the buffers that hold engine and helper storage.
- **Put configurations in read-only memory** where the platform allows: construct them in `const` context (RS-012), so that they live in flash or ROM rather than RAM. Read-only memory is far less exposed to upsets, and is often separately protected.
- **Treat an uncorrectable memory error as a reset condition** of the application, through the platform's trap handler. Aldis's engines are restartable from their initial state, and COP-1's own recovery (FOP-1 initiation; FARM-1 lockout handling) re-establishes the link.
- **Where the platform cannot protect memory,** treat Aldis's long-lived state as a hazard of the mission's own analysis. Mitigations are periodic restart of the engines, or redundant instances with comparison.

## 4. Where this is tracked

- **GEN-015** ([design rules](02-design-rules.md) §1) states the assumption; it is verified by inspection of this document.
- **The hazard and failure-mode analysis** at the M2 exit ([M2 plan](milestones/m2-frame-codecs.md) WP-8) includes an upset in unprotected memory as a failure mode, with this assumption as its control.
- **Milestone 3 planning** ([roadmap](08-roadmap.md)) decides how the COP-1 engines' state is hardened against upsets. The options, in order of cost:
  - **(a)** rely on this assumption alone;
  - **(b)** keep long-lived state in its raw wire form and parse it on each use, so that a corrupted value fails the parse instead of being undefined;
  - **(c)** a checksum over the engine state, checked on each call, whose mismatch triggers the protocol's own recovery.
