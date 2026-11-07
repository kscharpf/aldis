# aldis-framework

**Deferred (roadmap §7); not part of v1.** The crate is a skeleton, and its FW requirements ([05](../../docs/05-framework.md)) are outside the v1 trace and the v1.0 gate.

Optional framework: transport and clock traits, and protocol stacks with a run loop that drives the engines of `aldis-link` and `aldis-cop1`. The application supplies the I/O; the framework sends the engines' output, arms their timers, and dispatches their events.

## Where it sits

Layer 3 (framework) of the [architecture](../../docs/01-architecture.md) §2. It depends on `aldis-core`, `aldis-link`, and `aldis-cop1`. Unlike Layers 0 to 2, it may have optional third-party dependencies, behind non-default features only (GEN-004).
