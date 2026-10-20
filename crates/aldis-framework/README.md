# aldis-framework

Optional framework: transport and clock traits, and protocol stacks with a run loop that drives the engines of `aldis-link` and `aldis-cop1`. The application supplies the I/O; the framework sends the engines' output, arms their timers, and dispatches their events.

**Status:** skeleton. The modules are declared but not implemented; they are implemented in Milestone 6 ([roadmap](../../docs/08-roadmap.md) §1). It is Rust-only in v1.

## Where it sits

Layer 3 (framework) of the [architecture](../../docs/01-architecture.md) §2. It depends on `aldis-core`, `aldis-link`, and `aldis-cop1`. Unlike Layers 0 to 2, it may have optional third-party dependencies, behind non-default features only (GEN-004).

## Specification

- [docs/05-framework.md](../../docs/05-framework.md): requirement area FW.
- [docs/09-open-issues.md](../../docs/09-open-issues.md): OI-25 (resolved: the trace format of FW-013 is defined in M6, compatible with the reference application's golden traces).
- [docs/README.md](../../docs/README.md): the index of the specification.
