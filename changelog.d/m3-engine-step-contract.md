### Changed
- Breaking: the engine contract of `docs/03-api-model.md` §5–§8 is now a pure step function over plain-data state that returns the next state and a bounded action list (API-050…API-062, API-083), in place of output queues drained by `poll_*` calls. The deadline is a pure query, FARM-1 buffer availability is an input (COP-033), and SDLS is applied by the caller around an engine's frames, in the order of API-074 (OI-56).
- `docs/standards/cop1.md`, `docs/04-engines.md`, `docs/05-framework.md` (FW-014), `docs/profiles/rust.md` (RS-022, RS-023), `docs/profiles/c-abi.md` (CABI-040), and the glossary follow the new contract.

### Removed
- Requirements withdrawn in the specification: API-051, API-081 (the output queues), COP-032 (the internal FARM-1 buffer model), RS-014 and CABI-041 (security hooks inside engines).
