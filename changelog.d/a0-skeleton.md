### Added
- `refapp/`: workspace skeleton of the reference application (A0 WP-2): `refapp-scenario` (generated scenario constants and the node interface), empty `refapp-flight` (`no_std`, no heap) and `refapp-ground` nodes, and `refapp-harness` (scheduler and link model, golden-trace writer, canonical-form check, comparator, scenario schema and code generator). The scenario file `refapp/scenario/scenario.json` and the header-only golden trace `refapp/golden/a0-empty.jsonl`. See [refapp/README.md](refapp/README.md).
- `cargo xtask refapp-deps` (also in `cargo xtask ci`): `refapp/` uses Aldis only through the `aldis` facade and forbids unsafe code (APP-100, APP-105).
- CI job `refapp`: fmt, Clippy, tests, the `thumbv7em-none-eabihf` build of `refapp-flight`, generated-file and golden-trace checks; `cargo deny` also covers `refapp/` (APP-106).

### Changed
- `cargo xtask trace` reads `// req:` annotations in `refapp/` and the requirement IDs in golden-trace headers; `spec-lint` checks requirement IDs in `.jsonl` files (APP-108).
