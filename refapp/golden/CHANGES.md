# Golden-trace changes

Every change to a released golden trace in this directory bumps the case's `trace_version` in `refapp/scenario/scenario.json` and adds a row here with the reason (APP-087, [docs/reference-application.md](../../docs/reference-application.md) §7.5). Golden traces are whole-system regression baselines, not conformance vectors (APP-088).

| Case | Trace version | Date | Reason |
|---|---|---|---|
| `a0-empty` | 1 | 2026-10-19 | First version: header only (A0 skeleton self-test) |
