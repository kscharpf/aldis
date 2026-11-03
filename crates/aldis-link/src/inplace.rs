//! Building an engine in place, in the caller's storage (issue #240; CABI-040, GEN-012).
//!
//! `Engine::new(config)` returns the engine by value. An engine with large octet buffers (the
//! packet queue, the data field zone) then costs its own size on the stack at least, once for
//! the value and once for each move into the storage, which breaks the stack budget of the C ABI
//! at useful capacities. The pattern below builds it where it will live.
//!
//! An engine that follows the pattern is a `repr(C)` struct of exactly two fields, in this order:
//!
//! - the **head** (`EngineHead<P>`): the configuration, the counters, and every other small
//!   field. It is built by value from the configuration by `Engine::new_head(config)`, which
//!   makes the same checks as `Engine::new` and returns the same error.
//! - the **buffers** (`EngineBuffers<..>`): plain arrays of octets. All-zero octets are the empty
//!   state, so a caller that has no `MaybeUninit` can zero them. `EngineBuffers::ZERO` is the
//!   value that `Engine::new` uses.
//!
//! `Engine::HEAD_OFFSET` and `Engine::BUFFERS_OFFSET` are the offsets of the two fields
//! (`offset_of!`). To build the engine at `p: *mut Engine`, write the head at
//! `p.byte_add(HEAD_OFFSET)` and zero `size_of::<EngineBuffers>()` octets at
//! `p.byte_add(BUFFERS_OFFSET)`. This needs `unsafe`, so it lives in `aldis-ffi`, which does it
//! once for every engine (`handle::init_with`); `aldis-link` stays free of `unsafe` (GEN-070).
//! Only the head is ever a by-value temporary, and it is small.
//!
//! Engines that follow the pattern: the `PacketGenerator`, the `AccessGenerator`, the
//! `BitstreamGenerator`, the `PacketExtractor`, and the `ChannelMux` (whose Insert Zone slot is
//! stored XOR its idle pattern, and whose frame counts, zero at the start, move to the buffers
//! to keep the head small). The `FrameReceiver` holds no octet buffer
//! (its state is the configuration, the counts, and the counters), so it needs no split. The
//! others (MapSegmenter, MapReassembler, CltuPacker) take the same split when their C handles
//! are built (M4 WP-7d): the rule is to move the octet arrays, and only those, into
//! `EngineBuffers`.
